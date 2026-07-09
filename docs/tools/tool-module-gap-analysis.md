# 工具模块欠缺文档

本文用于指导后续工具模块开发与完善。

当前版本已按本 worktree 的代码重新核对。重点是避免后续执行者把不存在的工具、过期的 feature 状态和未落地的契约当成事实。

## 本次文档完善 checklist

- [x] 在隔离 worktree 中处理本文档。
- [x] 校准 `jyowo-harness-tool` feature 状态：当前 `default = []`，`builtin-toolset` 不是 tool crate 自身默认 feature。
- [x] 校准 `BuiltinToolset::Default` 注册清单：当前没有 Git builtin 工具，也没有 brokered platform builtin 工具。
- [x] 校准实际 descriptor 名称：`memory`、`skills_list`、`skills_view`、`skills_invoke`、`execute_code` 是小写或 snake case；MiniMax / Seedance descriptor 名称不带 `Tool` 后缀。
- [x] 补齐 ToolPool 装配链路：capability、service route、tenant policy、profile、deferred partition、runtime append。
- [x] 补齐 runtime 注入工具清单：`tool_search`、`background_agent`、`agent_team`、`agent`、team control tools。
- [x] 补齐 MCP wrapper 的 schema、authorization、trust、cancel 行为。
- [x] 补齐 plugin sidecar tool 的 manifest、schema、trust、sandbox 行为。
- [x] 补齐 tool search / deferred tools 的 output schema、scorer、delta 行为。
- [x] 重新标注过期项：默认工具集已默认开启、brokered platform 已实现、Git 工具已存在、`ToolDescriptorMetadata` 已存在，这些在当前代码中不成立。
- [x] 建立自动文档检查：本文中的工具名、feature、runtime 注入工具名必须能从代码生成或校验。

## 适用范围

本文覆盖当前已梳理到的工具相关模块：

- `crates/jyowo-harness-contracts/src/tool.rs`
- `crates/jyowo-harness-contracts/src/enums.rs`
- `crates/jyowo-harness-contracts/src/capability.rs`
- `crates/jyowo-harness-contracts/src/deferred_tools.rs`
- `crates/jyowo-harness-contracts/src/runtime_execution_status.rs`
- `crates/jyowo-harness-contracts/src/tool_profile.rs`
- `crates/jyowo-harness-tool/Cargo.toml`
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
- `crates/jyowo-harness-tool-search/src/*.rs`
- `crates/jyowo-harness-tool-search/src/backends/*.rs`
- `crates/jyowo-harness-mcp/src/wrapper.rs`
- `crates/jyowo-harness-mcp/src/authorization.rs`
- `crates/jyowo-harness-mcp/src/registry.rs`
- `crates/jyowo-harness-plugin/src/manifest.rs`
- `crates/jyowo-harness-plugin/src/cargo_extension.rs`
- `crates/jyowo-harness-plugin/src/capability.rs`
- `crates/jyowo-harness-subagent/src/lib.rs`
- `crates/jyowo-harness-engine/src/engine.rs`
- `crates/jyowo-harness-engine/src/turn.rs`
- `crates/jyowo-harness-sdk/src/harness.rs`
- `crates/jyowo-harness-sdk/src/harness/session_runtime.rs`
- `crates/jyowo-harness-sdk/src/harness/tool_pool.rs`
- `crates/jyowo-harness-sdk/src/lib.rs`
- `apps/desktop/src/features/conversation/evidence/ToolInvocationCard.tsx`
- `apps/desktop/src/features/conversation/timeline/*tool*`
- `apps/desktop/src/features/conversation/timeline/pending-tool-permission.ts`
- `scripts/check-tool-network-broker-boundary.mjs`
- `scripts/check-tool-network-broker-boundary.test.mjs`
- `crates/jyowo-harness-tool/tests/*.rs`
- `crates/jyowo-harness-tool-search/tests/*.rs`
- `crates/jyowo-harness-contracts/tests/*tool*`
- `crates/jyowo-harness-sdk/tests/*tool*`
- `crates/jyowo-harness-engine/tests/subagent_tool_feature.rs`

本文不是最终设计文档。它是欠缺清单和执行索引。

## 当前工具系统主线

### 核心执行链路

工具执行不是“拿到输入就执行”。主线是：

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

关键约束：

- `Tool::descriptor()` 声明工具契约。
- `Tool::validate()` 校验输入。
- `Tool::plan()` 生成权限计划。
- `Tool::execute_authorized()` 只接受已授权输入。
- `AuthorizationTicketClaims` 绑定 `tenant_id`、`session_id`、`run_id`、`tool_use_id`、`tool_name`、`action_plan_hash`。
- `TicketLedger::consume()` 检查 ticket 是否存在、是否过期、是否已消费、claims 是否匹配。
- `AuthorizedToolInput::new()` 再校验 `tool_use_id`、`tool_name`、canonical action plan hash。

这条链路的设计意图是：批准的是某个具体 action plan，执行时不能换工具、换输入或换资源。

### 注册和工具发现

当前结构：

- `ToolRegistry` 持有 `BTreeMap<String, RegisteredTool>`。
- `ToolRegistryBuilder` 按 `BuiltinToolset` 注册工具。
- `ToolRegistrySnapshot` 提供只读快照、descriptor 列表、group 查询、generation、journal authority 查询。
- `ToolPool` 基于 registry snapshot 生成本轮工具池。
- `jyowo-harness-tool-search` 提供 tool search 和 deferred tools 扩展，不替代 registry。
- SDK、MCP、plugin、subagent、team runtime 会向 registry 或 ToolPool 追加工具。

shadow 规则已经存在：

- builtin 与 builtin 重名时保留已有。
- builtin 与 plugin / MCP / skill 重名时，builtin 保留；如果 incoming 是 builtin，会替换非 builtin。
- 非 builtin 重名时，`AdminTrusted` 高于 `UserControlled`。
- 非 builtin 且同 trust 重名时保留已有。
- 每次 shadow 会记录 `ShadowedRegistration`，包含 kept、rejected、reason、时间。

仍缺：

- [x] shadowed 记录暂不进入 UI / telemetry / event stream；当前只作为 registry 内部审计记录。
- [x] builtin / plugin / MCP / skill / runtime appended tool 的重名矩阵已有 registry / pool 快照测试。

### ToolPool 装配过滤

registry 中存在的工具不等于本轮会进入 prompt 或可执行工具池。

SDK session 装配当前按以下顺序收窄工具：

1. `filter_unavailable_tools()`：按 descriptor 的 `required_capabilities` 过滤 capability 缺失工具。
2. `filter_unrouted_service_tools()`：按 descriptor 的 `service_binding` 和 provider capability routes 过滤没有路由的 provider service 工具。
3. `apply_tenant_tool_filter()`：按 tenant policy 的 allowed tools 过滤。
4. `ToolPoolFilter::from_profile()`：按 `ToolProfile` 过滤 group、MCP、plugin、allowlist、denylist。
5. `ToolPool::assemble()`：解析 dynamic schema，并按 tool search mode 拆分 always-loaded / deferred。
6. SDK 或 engine 再追加 runtime tools，例如 `tool_search`、`background_agent`、`agent_team`、subagent `agent`。
7. team coordinator runtime 会用独立 `ToolPool` 追加 team control tools。

审计时要区分：

- registry-visible
- pool-visible
- prompt-visible
- deferred but materializable
- runtime-appended
- team-coordinator-only

### 工具 journal authority

工具不能任意写 journal 事件。orchestrator 会按 `ToolJournalAuthority` gate `ToolEvent::Journal`：

- `Bash`、`Diagnostics`、`ProcessStart` 使用 `ToolJournalAuthority::Sandbox`。
- `Clarify` 使用 `ToolJournalAuthority::Clarification`。
- `execute_code` 使用 `ToolJournalAuthority::ExecuteCode`。
- 其他工具默认 `ToolJournalAuthority::None`。

未授权的 journal event 会返回 `PermissionDenied`，不会 emit 到 event stream。

已有覆盖：

- [x] `crates/jyowo-harness-tool/tests/orchestrator.rs` 覆盖非 owner clarification / sandbox / execute_code event 被拒绝。
- [x] `crates/jyowo-harness-tool/tests/registry_pool.rs` 覆盖 sandbox authority 和 runtime same-name tool 不覆盖已有 authority。

仍缺：

- [x] 按默认工具集输出完整 authority 快照。
- [x] 对新增 runtime appended 工具明确 authority 策略：runtime appended 默认 `None`，同名不覆盖已有 authority。

## feature flags

`crates/jyowo-harness-tool/Cargo.toml` 当前 feature：

```toml
default = []
skill-tools = []
builtin-toolset = ["jyowo-harness-permission/dangerous"]
programmatic-tool-calling = ["builtin-toolset", "jyowo-harness-sandbox/code-runtime"]
minimax-tools = ["builtin-toolset", ...]
seedance-tools = ["builtin-toolset", ...]
```

当前事实：

- `jyowo-harness-tool` 自身默认不启用 `builtin-toolset`。
- 直接依赖 `jyowo-harness-tool` 且不显式启用 feature 时，`ToolRegistry::builder().build()` 的 `BuiltinToolset::Default` 不会注册基础 builtin。
- `jyowo-harness-sdk` 的默认 feature 包含 `builtin-toolset`，并转发到 `jyowo-harness-tool/builtin-toolset`。
- `apps/desktop/src-tauri/Cargo.toml` 对 `jyowo-harness-tool` 使用 `default-features = false`，但手动启用 `builtin-toolset`、`minimax-tools`、`seedance-tools`。
- `programmatic-tool-calling`、`minimax-tools`、`seedance-tools` 仍是可选 feature。
- `BuiltinToolset::Skills` 只要启用 `builtin-toolset` 或 `skill-tools` 任一 feature，就能注册 skill 工具。

feature 欠缺项：

- [x] 已核对 tool crate 当前 `default = []`。
- [x] 已核对 SDK 默认 feature 会启用 `builtin-toolset`。
- [x] 已核对 desktop 手动启用需要的 tool features。
- [x] 已决定保持 `jyowo-harness-tool` 的 `default = []`，直接消费者必须显式启用 `builtin-toolset`。
- [x] 新增完整快照测试：默认工具集名称和数量稳定。
- [x] 新增编译路径测试：`default-features = false` 时 builder 行为符合预期。
- [x] 新增编译路径测试：只启用 `skill-tools` 时 `BuiltinToolset::Skills` 可注册 `skills_list`、`skills_view`、`skills_invoke`。

## 当前已存在的工具清单

### `BuiltinToolset::Default`

这些工具由 `BuiltinToolset::Default` 注册，前提是编译启用了 `builtin-toolset`。

文件和搜索：

- `FileRead` -> `crates/jyowo-harness-tool/src/builtin/read.rs`
- `FileEdit` -> `crates/jyowo-harness-tool/src/builtin/edit.rs`
- `FileWrite` -> `crates/jyowo-harness-tool/src/builtin/write.rs`
- `ListDir` -> `crates/jyowo-harness-tool/src/builtin/list_dir.rs`
- `Grep` -> `crates/jyowo-harness-tool/src/builtin/grep.rs`
- `Glob` -> `crates/jyowo-harness-tool/src/builtin/glob.rs`
- `ReadBlob` -> `crates/jyowo-harness-tool/src/builtin/read_blob.rs`

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
- `memory` -> `crates/jyowo-harness-tool/src/builtin/memory.rs`
- `TaskStop` -> `crates/jyowo-harness-tool/src/builtin/task_stop.rs`
- `skills_list` -> `crates/jyowo-harness-tool/src/builtin/skills.rs`
- `skills_view` -> `crates/jyowo-harness-tool/src/builtin/skills.rs`
- `skills_invoke` -> `crates/jyowo-harness-tool/src/builtin/skills.rs`

当前不存在于 `BuiltinToolset::Default`：

- [x] 未发现 `GitStatus`、`GitDiff`、`GitShow`、`GitLog`、`GitStage`、`GitCommit`、`GitBranch`、`GitPull`、`GitPush` 的 builtin 实现或注册。
- [x] 未发现 `Worktree`、`Session`、`Artifact`、`BrowserUse`、`ComputerUse`、`ImageGeneration`、`NotebookEdit`、`LSP`、`Automation`、`Workflow` 的 brokered platform builtin 实现或注册。

### 可选程序化执行工具

需要 `programmatic-tool-calling` feature。

- `execute_code` -> `crates/jyowo-harness-tool/src/builtin/execute_code.rs`

### MiniMax 工具

需要 `minimax-tools` feature。descriptor 名称如下：

- `MiniMaxTextToImage`
- `MiniMaxImageToImage`
- `MiniMaxTextToVideo`
- `MiniMaxImageToVideo`
- `MiniMaxFirstLastFrameToVideo`
- `MiniMaxSubjectReferenceVideo`
- `MiniMaxVideoGenerationQuery`
- `MiniMaxVideoTemplate`
- `MiniMaxVideoTemplateQuery`
- `MiniMaxTextToSpeech`
- `MiniMaxTextToSpeechAsync`
- `MiniMaxTextToSpeechAsyncQuery`
- `MiniMaxVoiceClone`
- `MiniMaxVoiceDesign`
- `MiniMaxListVoices`
- `MiniMaxDeleteVoice`
- `MiniMaxLyricsGeneration`
- `MiniMaxMusicGeneration`
- `MiniMaxMusicCoverPreprocess`
- `MiniMaxFileUpload`
- `MiniMaxFileList`
- `MiniMaxFileRetrieve`
- `MiniMaxFileDelete`
- `MiniMaxModelsList`
- `MiniMaxModelRetrieve`
- `MiniMaxResponses`
- `MiniMaxResponsesInputTokens`
- `MiniMaxAnthropicMessages`
- `MiniMaxAnthropicCountTokens`
- `MiniMaxAnthropicModelsList`
- `MiniMaxAnthropicModelRetrieve`

实现文件：

- `crates/jyowo-harness-tool/src/builtin/minimax.rs`
- `crates/jyowo-harness-tool/src/provider_media.rs`
- `crates/jyowo-harness-tool/src/provider_minimax.rs`

### Seedance 工具

需要 `seedance-tools` feature。descriptor 名称如下：

- `SeedanceTextToVideo`
- `SeedanceImageToVideo`
- `SeedanceVideoGenerationQuery`

实现文件：

- `crates/jyowo-harness-tool/src/builtin/seedance.rs`
- `crates/jyowo-harness-tool/src/provider_media.rs`

### 工具搜索和 deferred tools

这部分不是普通 builder 注册工具，而是 ToolPool 扩展能力。

相关文件：

- `crates/jyowo-harness-tool-search/src/search_tool.rs`
- `crates/jyowo-harness-tool-search/src/runtime.rs`
- `crates/jyowo-harness-tool-search/src/backend.rs`
- `crates/jyowo-harness-tool-search/src/backends/anthropic.rs`
- `crates/jyowo-harness-tool-search/src/backends/inline.rs`
- `crates/jyowo-harness-tool-search/src/coalescer.rs`
- `crates/jyowo-harness-tool-search/src/delta.rs`
- `crates/jyowo-harness-tool-search/src/scorer.rs`
- `crates/jyowo-harness-contracts/src/deferred_tools.rs`

当前能力：

- [x] `tool_search` 是 runtime tool，名称为 `tool_search`。
- [x] `tool_search` descriptor 包含 output schema。
- [x] `tool_search` 预算 metric 是 `Bytes`，limit 是 32 KiB。
- [x] 支持 `select:Read,Edit,Grep`、普通关键词、`+required` 查询。
- [x] 支持 Anthropic tool reference backend 和 inline reinjection backend。
- [x] deferred delta attachment 记录 added / removed / source / timestamp / initial。
- [x] scorer 已覆盖 name parts、description、search_hint、MCP name parsing、discovered penalty。

仍缺：

- [x] 搜索结果解释具体命中字段。
- [x] materialization reason 已进入 `tool_search` 结构化输出。
- [x] required capabilities 已参与 scorer 加权。

### SDK / engine / team runtime 注入工具

这些工具不在 `jyowo-harness-tool` 的默认 builder 注册列表中，但会进入实际运行。

普通 session runtime：

- `tool_search` -> `crates/jyowo-harness-tool-search/src/search_tool.rs`
- `background_agent` -> `crates/jyowo-harness-sdk/src/harness/tool_pool.rs`
- `agent_team` -> `crates/jyowo-harness-sdk/src/harness/tool_pool.rs`
- `agent` -> `crates/jyowo-harness-subagent/src/lib.rs`

team coordinator runtime：

- `dispatch` -> `crates/jyowo-harness-sdk/src/lib.rs`
- `message` -> `crates/jyowo-harness-sdk/src/lib.rs`
- `pause_worker` -> `crates/jyowo-harness-sdk/src/lib.rs`
- `resume_worker` -> `crates/jyowo-harness-sdk/src/lib.rs`
- `spawn_worker` -> `crates/jyowo-harness-sdk/src/lib.rs`
- `stop_team` -> `crates/jyowo-harness-sdk/src/lib.rs`
- `team_status` -> `crates/jyowo-harness-sdk/src/lib.rs`

runtime 注入边界：

- [x] `background_agent` 需要 tenant allowlist、run policy allow、`jyowo.background_agent.starter` capability。
- [x] `agent_team` 需要 tenant allowlist、run policy allow、`agents-team` feature 和 team runner capability。
- [x] `agent` 由 engine 在启用 subagent tool 时追加；缺少 `SubagentRunner` capability 时 engine 会安装默认 runner。
- [x] team control tools 当前 `input_schema` 只有 `{ "type": "object" }`。
- [x] team control tools 当前 `output_schema: None`。
- [x] team control tools 当前 plan resources 为空，workspace / network access 均为 none。

### MCP / plugin 生成工具

这些工具由外部注册源生成 descriptor，并进入同一套 registry / pool / orchestrator 流程。

MCP wrapper：

- [x] descriptor 使用 canonical harness name，`display_name` 保留上游 tool name。
- [x] `input_schema` 保留上游 MCP tool schema。
- [x] `output_schema` 保留上游 MCP tool schema。
- [x] `origin` 使用 `ToolOrigin::Mcp(...)`。
- [x] `search_hint` 使用 description。
- [x] MCP input 必须是 object，并用 JSON Schema validator 校验。
- [x] MCP tool call plan 使用 `PermissionCheck::AskUser`。
- [x] MCP transport/resource/prompt/sampling 有各自 authorization 建模。
- [x] MCP progress 会映射为 progress / heartbeat。
- [x] interrupt 会向上游发送 cancel 并等待 ack；ack 超时会标记连接 unhealthy。

plugin sidecar tool：

- [x] manifest 使用 `#[serde(deny_unknown_fields)]`。
- [x] manifest tool entry 当前只声明 `name`、`destructive`、`input_schema`。
- [x] manifest tool entry 当前不支持 `output_schema`。
- [x] sidecar descriptor 的 `output_schema` 当前为 `None`。
- [x] sidecar tool 使用 manifest `input_schema` 做 JSON Schema validate。
- [x] sidecar tool plan 使用 `PermissionCheck::AskUser`。
- [x] sidecar execution channel 是 `ExternalCapability { capability: Custom("plugin_sidecar") }`。
- [x] `UserControlled` plugin 不能注册 destructive tool。
- [x] `UserControlled` plugin 不能注册 remote MCP transports。
- [x] sandboxed sidecar 使用 workspace readonly、network none、workspace-only scope。

## 能力依赖清单

工具注册成功不代表能执行成功。很多工具依赖 `ToolContext.cap_registry` 中的 capability，或依赖 SDK session assembly 的 provider route。

| Capability / runtime | 影响工具 | 当前状态 / 需要补齐的开发工作 |
|---|---|---|
| `ToolNetworkBrokerCap` | `WebFetch`、MiniMax、Seedance | `WebFetch`、MiniMax、Seedance 执行路径走 broker。MiniMax / Seedance 不把它列入 descriptor `required_capabilities`，但执行时会读取 broker。需要持续用边界测试防回归。 |
| `WebSearchBackend` | `WebSearch` | `WebSearch` 走 backend，不走 `ToolNetworkBrokerCap`。需要明确 backend 是否必须实现同等权限、redaction、审计。 |
| `DiagnosticsRunnerCap` | `Diagnostics` | 需要确认 runner 的工作目录、命令来源、输出清洗、超时策略。 |
| `ClarifyChannelCap` | `Clarify` | 需要确认 UI 通道存在时才展示工具，或执行前给出清晰错误。 |
| `MemoryToolRuntimeCap` | `memory` | 需要确认记忆写入、可见性、线程设置、敏感内容过滤。 |
| `UserMessengerCap` | `SendMessage` | 需要确认 outbound message 通道、目标、审计和失败语义。 |
| `TodoStoreCap` | `Todo` | 需要确认 todo 合并、状态约束、持久化和并发更新语义。 |
| `RunCancellerCap` | `TaskStop` | 需要确认 stop 作用域、重复停止和当前 run 终止语义。 |
| `SkillRegistryCap` | `skills_list`、`skills_view`、`skills_invoke` | 需要确认 skill 可见性、渲染、参数校验和来源边界。 |
| `ContextPatchSinkCap` | `skills_invoke` | 需要确认 skill 注入后的 context patch 生命周期、去重和撤销语义。 |
| `RunScopedProcessRegistryCap` | `ProcessStart`、`ProcessRead`、`ProcessStop` | 需要确认 process id 作用域、输出读取、停止语义和 redaction。 |
| `ProviderCredentialResolverCap` | MiniMax、Seedance | descriptor 会声明该 capability。需要确认凭证解析、provider 限制、错误脱敏和 route 选择。 |
| `BlobWriterCap` | MiniMax / Seedance media 或 async query 工具 | descriptor 对产出 media/blob 的工具会声明它。需要确认写入、retention、content-type 和 artifact 引用。 |
| `BlobReaderCap` | `ReadBlob`、orchestrator offload 读取 | 需要确认 blob 读取权限、offset/limit、过期和二进制输出策略。 |
| `OffloadedBlobAuthorizerCap` | `ReadBlob` | 需要确认 offloaded blob 只允许当前租户、会话、run 或授权范围读取。 |
| `CodeRuntimeCap` | `execute_code` | 需要确认程序化执行 runtime、资源限制和语言隔离。 |
| `EmbeddedToolDispatcherCap` | `execute_code` | 需要确认代码内嵌工具调用的权限继承、审计和循环限制。 |
| `ToolSearchRuntimeCap` | `tool_search` | descriptor 当前 `required_capabilities` 为空，但执行时会读取 runtime capability。需要确认 SDK 注入和 disabled 模式行为。 |
| `ToolCapability::SubagentRunner` | `agent` | 需要确认 subagent lifecycle、输出契约、权限继承和失败恢复。 |
| `jyowo.background_agent.starter` | `background_agent` | 需要确认后台 agent capability readiness、权限模式、session snapshot 和输出契约。 |
| `jyowo.agent_team.runner` | `agent_team` | 需要确认 team runner readiness、成员生命周期、结果契约和停止语义。 |
| sandbox backend | `Bash`、`ProcessStart`、`execute_code`、部分文件操作 | 需要确认 sandbox policy、workspace scope、超时、interrupt 均生效。 |
| blob store | 大输出、offload、`ReadBlob` | 需要确认 offload 后可读、权限、retention、redaction。 |

provider route gating：

- [x] MiniMax / Seedance 部分 descriptor 会设置 `service_binding`。
- [x] SDK session assembly 会用 provider capability routes 过滤没有启用 route 的 service-bound tools。
- [x] `ProviderCredentialResolverCap` 存在不保证 provider 工具会出现在 prompt 中。
- [x] service binding / route gating 已写入 MiniMax / Seedance provider 工具 contract test 矩阵。

## 欠缺项总览

### G1. tool crate 默认 feature 与下游配置需要定案

当前状态：

- `crates/jyowo-harness-tool/Cargo.toml` 当前是 `default = []`。
- `BuiltinToolset::Default` 的基础内置工具注册都在 `#[cfg(feature = "builtin-toolset")]` 下。
- `jyowo-harness-sdk` 默认 feature 会启用 `builtin-toolset`。
- desktop 手动启用 `builtin-toolset`、`minimax-tools`、`seedance-tools`。

影响：

- 直接使用 `jyowo-harness-tool` 的消费者如果没有显式启用 `builtin-toolset`，会得到没有基础 builtin 的 registry。
- SDK 默认路径可用，但 tool crate 直接路径不一定可用。

处理方向：

- [x] 文档已校准当前 feature 事实。
- [x] 决定 `jyowo-harness-tool` 保持 `default = []`。
- [x] 若保持 `default = []`，需要文档和 compile test 明确这是设计。
- [x] 默认工具清单快照已覆盖当前设计。

### G2. Git / brokered platform 工具清单是历史规划，不是当前实现

当前状态：

- 当前代码没有 `crates/jyowo-harness-tool/src/builtin/git.rs`。
- 当前代码没有 `crates/jyowo-harness-tool/src/builtin/brokered_platform.rs`。
- 当前 builder 不注册 Git 工具。
- 当前 builder 不注册 `Worktree`、`Session`、`Artifact`、`BrowserUse`、`ComputerUse`、`ImageGeneration`、`NotebookEdit`、`LSP`、`Automation`、`Workflow`。

处理方向：

- [x] 文档已移除它们作为“当前已存在工具”的表述。
- [x] 文档已把它们改为“当前未实现 / 未来如新增需补契约”。
- [ ] 如果要引入这些工具，先新增真实 implementation、feature gate、registry 快照和 host contract。

### G3. 多数工具缺 output schema

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
- `agent`、`background_agent`、`agent_team`、team control tools 当前未声明 output schema。
- plugin sidecar tool 当前不支持 manifest output schema。

优先级：

1. `FileRead`
2. `FileWrite`
3. `FileEdit`
4. `ListDir`
5. `Grep`
6. `Glob`
7. `ReadBlob`
8. `ProcessStart`
9. `ProcessRead`
10. `ProcessStop`
11. `Diagnostics`
12. `Bash`
13. `WebFetch`
14. `WebSearch`
15. `memory`
16. `Todo`
17. `skills_list`
18. `skills_view`
19. `skills_invoke`
20. `SendMessage`
21. `TaskStop`
22. `execute_code`
23. runtime 注入工具
24. provider 工具

处理方向：

- [x] 给基础 builtin、MiniMax、Seedance、`tool_search` 填充 output schema。
- [x] 对结构化输出建立 schema builder / provider output schema helper。
- [x] 对基础 builtin、MiniMax、Seedance、`tool_search` output schema 增加 contract test。
- [x] UI 只依赖 schema 中声明的字段。
- [x] 对 Text / Structured / Mixed / Blob / Offloaded 输出写清楚 schema 表达方式：运行事件和 conversation projection 使用 `resultKind: "text" | "structured" | "mixed" | "blob" | "offloaded"`，具体内容只通过已声明的 safe summary、attachment/blob reference 或 offload metadata 展示。

### G4. input schema 已默认拒绝未知字段

当前 `object_schema()`：

```rust
json!({
    "type": "object",
    "required": required,
    "properties": properties
})
```

已统一设置：

```json
"additionalProperties": false
```

需要区分两层契约：

- `input_schema` 是 descriptor contract，主要影响 prompt、SDK、UI 和 contract test。
- orchestrator 会先对 descriptor `input_schema` 跑 JSON Schema validator。
- 实际执行路径是 `ToolOrchestrator -> descriptor schema validate -> tool.validate()`。
- MCP wrapper 和 plugin sidecar tool 会使用 JSON Schema validator；多数 builtin 不是这条路径。

已知例外：

- [x] 基础 builtin 的 `object_schema()` 已设置 `additionalProperties: false`。
- [x] `background_agent`、`agent_team` input schema 已设置 `additionalProperties: false`。
- [x] MCP wrapper 和 plugin sidecar tool 会按 descriptor schema 做 JSON Schema validate。
- [x] orchestrator 已统一执行 descriptor schema validation，并覆盖未知字段失败测试。

仍缺：

- [x] `agent` input schema 已设置 `additionalProperties: false`。
- [x] team control tools input schema 已按工具声明，并设置 `additionalProperties: false`。

### G5. permission plan 的资源建模需要逐工具审计

当前 helper `generic_action_plan()` 使用空资源：

```rust
Vec::new(),
WorkspaceAccess::None,
NetworkAccess::None,
```

已建模：

- [x] 文件读类工具使用 `ActionResource::FileRead`。
- [x] 文件写类工具使用 `ActionResource::FileWrite`。
- [x] `Bash`、`ProcessStart`、`Diagnostics` 使用 `ActionResource::Command`。
- [x] `WebFetch`、`WebSearch`、MiniMax、Seedance 使用 `ActionResource::Network` 或网络访问计划。
- [x] `SendMessage` 使用 `ActionResource::Network`、`ToolCapability::UserMessenger` 和 `ExternalCapability` execution channel。
- [x] MCP authorization 已建模 MCP transport/tool/resource/prompt/sampling 等资源。

已审计的 `generic_action_plan()` 使用者：

- [x] `ProcessRead`
- [x] `ProcessStop`
- [x] `Clarify`
- [x] `skills_list`
- [x] `skills_view`
- [x] `skills_invoke`
- [x] `Todo`
- [x] `ReadBlob`
- [x] `memory`
- [x] `TaskStop`
- [x] `execute_code`

仍缺或需细化：

- [x] `ActionResource` enum 已补 Memory / Process / TeamControl / BlobRead 等资源变体。
- [x] `memory` 写入已建模 memory subject。
- [x] `ProcessRead` / `ProcessStop` 已建模 process resource。
- [x] team control tools 已建模 `ActionResource::TeamControl`。
- [x] `ReadBlob` 已补 `BlobRead` resource。
- [x] permission review details 已有直接测试覆盖。

### G6. `ToolDescriptorMetadata` 当前不存在，实际可用信号是 descriptor 字段和 `search_hint`

当前 `ToolDescriptor` 字段包括：

- `name`
- `display_name`
- `description`
- `category`
- `group`
- `version`
- `input_schema`
- `output_schema`
- `dynamic_schema`
- `properties`
- `trust_level`
- `required_capabilities`
- `budget`
- `provider_restriction`
- `origin`
- `search_hint`
- `service_binding`

当前不存在独立的 `ToolDescriptorMetadata` 字段，也没有 `aliases`、`families`、`risk_level`、`effects`、`modalities`、`integration_source` 这些 descriptor 字段。

影响：

- tool search 主要依赖 name parts、description、search_hint、origin、group、defer 状态等信号。
- UI 不能从 descriptor 直接读取 risk/effects/modalities。
- 如果后续要补 metadata，需要先扩展 contracts。

处理方向：

- [x] 文档已校准当前 descriptor 字段。
- [x] 决定暂不扩展 `ToolDescriptor` 增加 aliases / risk / effects / modalities；当前用 `search_hint` 覆盖检索缺口。
- [x] 如果不扩展，至少为基础工具补 `search_hint`。
- [x] 新增 descriptor contract 测试，防止可检索信号回退。

### G7. 网络边界存在两套路径

当前情况：

- `WebFetch` 走 `ToolNetworkBrokerCap`。
- MiniMax 和 Seedance 走 `ToolNetworkBrokerCap`。
- `WebSearch` 走 `WebSearchBackend`。

影响：

- 如果 `WebSearchBackend` 内部没有走同等权限和 redaction 策略，网络边界会不一致。
- 静态边界测试可能覆盖不到 backend 内部实现。

处理方向：

- [x] 文档已明确 `WebSearchBackend` 是单独网络路径。
- [x] 明确 `WebSearchBackend` 作为单独 external capability 边界，不在 tool crate 内直接走 raw network。
- [x] 当前不调整 trait 或 runtime 实现；`WebSearchBackend` 作为独立 external capability 边界处理。
- [x] 如果不必须，补安全说明和单独 permission policy。
- [x] 扩展 `check-tool-network-broker-boundary` 覆盖 WebSearch backend 例外边界。
- [x] 增加 host、scheme、content-type、body-size 测试；redirect 安全在 provider media 下载测试中覆盖，WebFetch 本体走 broker 边界。

### G8. long-running、heartbeat、timeout 策略需要逐工具确认

当前 descriptor helper 默认：

```rust
long_running: None
```

`ToolOrchestrator` 已支持：

- `long_running_policy.stall_threshold`
- heartbeat
- `hard_timeout`
- interrupt

欠缺点：

- [x] 需要确认哪些工具应该声明 long-running：当前先覆盖 `Bash`、`ProcessStart`、`ProcessRead`、`ProcessStop`、`execute_code`；provider async 工具仍单独保留。
- [x] 需要确认 async query 型 provider 工具是否应该使用 heartbeat：MiniMax / Seedance async create/query/media generation 工具通过 descriptor `LongRunningPolicy` 使用 heartbeat 和 hard timeout。
- [x] 需要确认 process / sandbox / code runtime 的 timeout 来源和默认值：descriptor `LongRunningPolicy` 作为 orchestrator hard timeout / heartbeat 来源，sandbox 内部 timeout 继续由 sandbox 执行层处理。

处理方向：

- [x] 为 `Bash` 定义 timeout 和 heartbeat 策略。
- [x] 为 `ProcessStart` / `ProcessRead` / `ProcessStop` 定义 timeout 和 heartbeat 策略。
- [x] 为 `execute_code` 定义 timeout 和 sandbox interrupt 策略。
- [x] 为 MiniMax 视频、音乐、TTS async 工具定义长任务策略。
- [x] 为 Seedance 视频任务定义长任务策略。
- [ ] 增加超时、interrupt、partial progress 测试。

### G9. result budget 和 offload 需要按工具校准

当前 helper 默认：

- metric: chars
- overflow: offload
- preview head: 2000 chars
- preview tail: 2000 chars

例外：

- [x] `tool_search` 使用 `BudgetMetric::Bytes`，limit 为 32 KiB，并且已有 output schema。
- [x] result budget / offload 已有核心测试。
- [x] `ReadBlob` 已有 capability、授权、offset/limit 相关测试。

欠缺点：

- [x] 文件读、grep、web fetch、process output 的预算不应完全相同。
- [x] 二进制、图片、音频、视频、blob 输出需要更明确的预算表达：provider media 下载、base64/data URL 解码和 blob 写入均按 bytes 上限返回 `ResultTooLarge`。
- [x] provider 工具的 artifact 输出要明确是 inline、blob、URL 还是 mixed：async job 返回 mixed structured metadata，已完成媒体返回 mixed artifact/blob，pending 状态返回 structured provider 状态。

处理方向：

- [x] 给文本型大输出工具单独设置预算。
- [x] 给二进制 / provider media 工具设置 blob-first 策略。
- [x] 给 process output 设置 stdout/stderr 分离预算。
- [x] 给 WebFetch 设置 content-type aware budget。
- [x] 补 offload 写入与 `ReadBlob` 恢复读取之间的端到端集成测试。
  - [x] `FileRead` 使用 chars 大输出预算，`Grep` 使用 lines 预算，`ProcessRead` / `WebFetch` 使用 bytes 预算，并有 descriptor 快照测试。

### G10. dynamic schema 能力尚未充分使用

当前 `Tool::resolve_schema()` 默认返回静态 schema。descriptor helper 默认 `dynamic_schema: false`。

潜在需要 dynamic schema 的工具：

- `skills_invoke`：不同 skill 的参数不同。
- `MiniMax*`：不同模型支持的参数可能不同。
- `Seedance*`：不同模型支持的参数可能不同。
- runtime / host 层如果未来加入 workflow 或 automation，也可能需要 dynamic schema。

处理方向：

- [x] 明确哪些工具需要 dynamic schema：当前生产工具均未声明 `dynamic_schema: true`，`skills_invoke`、MiniMax、Seedance 继续使用静态 schema，动态 schema 作为未来 provider / skill 参数变体能力保留。
- [x] 为这些工具实现 `resolve_schema()`：当前无生产工具需要覆盖默认实现；Tool trait 默认返回 descriptor input schema，ToolPool 已有动态 schema assembly 测试。
- [ ] 给 schema resolver 加 context-aware cache。
- [x] 给动态 schema 加 contract test。
- [ ] UI 和 SDK 支持刷新动态 schema。

### G11. deferred tools 依赖可检索信号，但当前信号不足

当前已实现：

- [x] scorer 已使用 name parts、description、search_hint、origin、MCP name parsing、discovered penalty。
- [x] scorer 已有 MCP/camel/snake parsing、required-term、search_hint、description、discovered penalty 相关测试。
- [x] `tool_search` 已有 output schema contract 测试。

仍缺：

- [x] required capabilities 已进入 scorer 加权。
- [x] 搜索结果已经解释具体命中字段。
- [x] deferred delta 缺少面向用户的 reason。
- [x] 基础工具已有 `search_hint`。

处理方向：

- [x] 所有基础工具补 `search_hint`。
- [x] 评估 required capabilities 是否应进入 scorer 加权或过滤：当前进入加权，不做过滤。
- [x] 搜索结果解释包含命中字段。
- [x] deferred delta 中加入可读 reason。
- [x] 增加“文件任务命中文件工具”、“网络任务命中网络工具”、“图片任务命中 provider media 工具”的检索测试。

### G12. 插件和 MCP 工具生命周期需要补完整文档和测试

当前已有：

- [x] `register_from_plugin()`。
- [x] `deregister_from_plugin()`。
- [x] `deregister_mcp_tool()`。
- [x] MCP canonical name 相关函数。
- [x] shadowed registration 记录。
- [x] shadow resolution：builtin wins；非 builtin 高 trust wins；同 trust 保留已有。
- [x] MCP wrapper 保留上游 `input_schema` / `output_schema`。
- [x] MCP wrapper 使用 `ToolOrigin::Mcp(...)` 和 `search_hint`。
- [x] plugin sidecar tool 使用 JSON Schema validate。
- [x] plugin sidecar descriptor 使用 `ToolOrigin::Plugin { plugin_id, trust }`。
- [x] plugin manifest 当前不支持 tool `output_schema`。

欠缺点：

- [x] 插件工具 trust level 与 capability 许可边界需要更完整测试。
- [x] MCP 工具命名、注销、shadow 行为需要完整生命周期矩阵。
- [ ] plugin sidecar tool 当前没有 output schema。
- [x] shadowed 记录暂不暴露给 UI / telemetry。

处理方向：

- [x] 文档已写入 MCP wrapper 当前生命周期事实。
- [x] 文档已写入 plugin sidecar 当前生命周期事实。
- [x] 文档已明确 plugin manifest 当前不支持 output schema。
- [x] 补 builtin / plugin / MCP / skill / runtime appended 重名优先级快照。
- [x] 补 trust level 与 destructive 工具测试。
- [x] 补 capability not permitted 测试。
- [x] 决定 shadowed 暂不进入事件流。

### G13. UI 工具调用展示需要对齐后端契约

相关文件：

- `apps/desktop/src/features/conversation/evidence/ToolInvocationCard.tsx`
- `apps/desktop/src/features/conversation/evidence/ToolInvocationCard.test.tsx`
- `apps/desktop/src/features/conversation/timeline/tool-group-segment-view.tsx`
- `apps/desktop/src/features/conversation/timeline/tool-attempt-row.tsx`
- `apps/desktop/src/features/conversation/timeline/pending-tool-permission.ts`

欠缺点：

- output schema 缺失会让 UI 只能按 ad-hoc 字段渲染。
- permission review 的 resources 如果不完整，UI 无法展示真实风险。
- deferred tools delta 需要用户可理解的展示。
- runtime capability missing 需要单独状态。

处理方向：

- [x] UI 渲染基于 output schema 和 ToolResult 类型。
- [x] pending permission 展示 resources、scope、severity、confirmation。
- [x] capability missing 展示能力缺失，而不是普通失败。
- [x] deferred tools 展示新增/移除和原因。
- [x] 为每种 ToolResult 增加 UI 测试。

### G14. 测试矩阵需要从“有测试”升级为“按工具覆盖”

已有部分覆盖：

- [x] 默认工具注册关键名称。
- [x] ticket mismatch / plan hash mismatch 防护。
- [x] capability missing 的部分覆盖。
- [x] result budget / offload 核心行为。
- [x] `ReadBlob` capability、授权和窗口读取。
- [x] `tool_search` output schema、scorer、search hint。
- [x] MiniMax / Seedance 的部分 provider、network、authorization 测试。
- [x] ToolPool profile filter、dynamic schema assembly、runtime same-name 去重的部分覆盖。
- [x] provider capability route filter 的部分覆盖。
- [x] journal authority 的非 owner event 拒绝和默认 sandbox authority 的部分覆盖。
- [x] MCP wrapper schema validation、trusted annotation、cancel/interrupt 的部分覆盖。
- [x] plugin sidecar sandbox / capability / manifest 的部分覆盖。

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
  - [x] permission resource：`ActionResource::FileRead`。
  - [x] 输出 schema：content/text、path、size、truncated/offloaded、encoding。
  - [ ] 测试：正常读、缺文件、目录、越界路径、大文件 offload、redaction。

- [ ] `FileEdit`
  - [x] permission resource：`ActionResource::FileWrite`。
  - [ ] 明确 patch / old-new 字符串语义。
  - [x] 输出 schema：path、changed、diff、old_hash/new_hash。
  - [ ] 测试：唯一匹配、多重匹配、无匹配、越界路径、并发修改。

- [ ] `FileWrite`
  - [x] 当前实现语义是创建或覆盖。
  - [x] permission resource：`ActionResource::FileWrite`。
  - [x] 输出 schema：path、bytes_written、content_hash。
  - [ ] 测试：创建、覆盖、父目录不存在、越界路径、内容 hash。

- [ ] `ListDir`
  - [x] permission resource：`ActionResource::FileRead`。
  - [x] 输出 schema：entries、entry_type、size、modified。
  - [ ] 测试：空目录、大目录、隐藏文件、越界路径、排序。

- [ ] `Grep`
  - [x] permission resource：`ActionResource::FileRead`。
  - [ ] 明确 regex 语法、大小写、glob、limit。
  - [x] 输出 schema：matches、file、line、column、preview、truncated。
  - [ ] 测试：无匹配、多匹配、非法 regex、大输出 offload。

- [ ] `Glob`
  - [x] permission resource：`ActionResource::FileRead`。
  - [ ] 明确 glob 语法、忽略规则、排序。
  - [x] 输出 schema：paths、truncated。
  - [ ] 测试：递归、ignore、空结果、大结果。

- [ ] `ReadBlob`
  - [x] 已有 capability、授权、offset/limit 相关测试。
  - [ ] 明确 blob id、retention、权限。
  - [x] 输出 schema：blob_id、content、content_type、size。
  - [x] permission resource：当前走 `generic_action_plan()`，需要确认是否补 blob resource。
  - [ ] 测试：过期、二进制、offload 后端到端恢复读取。
  - [x] offload 后通过 `ReadBlob` 端到端恢复读取已有测试。

### 执行、诊断、进程工具

- [ ] `Bash`
  - [x] 当前 input schema：`command`、`cwd`。
  - [x] permission resource：`ActionResource::Command`。
  - [ ] 明确是否需要扩展 env、timeout。
  - [ ] sandbox policy 明确。
  - [x] output schema：exit_code、stdout、stderr、timed_out、truncated。
  - [ ] 测试：成功、非零退出、timeout、interrupt、redaction、cwd 越界。

- [ ] `Diagnostics`
  - [x] permission resource：`ActionResource::Command`。
  - [ ] runner capability 缺失错误可读。
  - [x] output schema：diagnostics、path、line、severity、source。
  - [ ] 测试：TypeScript、Cargo、空诊断、解析失败。

- [ ] `ProcessStart`
  - [x] 当前 input schema：`command`、`args`、`cwd`、`buffer_bytes`。
  - [x] permission resource：`ActionResource::Command`。
  - [ ] 明确前台/后台、terminal id、env、timeout。
  - [x] output schema：process_id、started、stdout_preview、stderr_preview。
  - [ ] 测试：成功启动、启动失败、重复启动、redaction。

- [ ] `ProcessRead`
  - [x] 当前 input schema：`process_id`、`max_bytes`。
  - [x] permission resource：当前走 `generic_action_plan()`，需要确认是否补 process resource。
  - [x] output schema：stdout、stderr、running、exit_code、offset。
  - [ ] 测试：运行中、已退出、未知进程、大输出、redaction。

- [ ] `ProcessStop`
  - [x] 当前 input schema：`process_id`。
  - [ ] 明确 graceful/kill 策略。
  - [x] permission resource：当前走 `generic_action_plan()`，需要确认是否补 process resource。
  - [x] output schema：stopped、signal、exit_code。
  - [ ] 测试：停止运行中进程、停止未知进程、重复停止。

- [ ] `execute_code`
  - [x] 只在 `programmatic-tool-calling` 下注册。
  - [ ] sandbox code runtime 权限明确。
  - [x] output schema：result、stdout、stderr、artifacts、timeout。
  - [ ] 测试：成功、异常、timeout、资源限制、危险代码隔离。

### 网络工具

- [ ] `WebFetch`
  - [x] 执行路径通过 `ToolNetworkBrokerCap`。
  - [x] 当前 input schema：`url`、`max_bytes`。
  - [x] permission resource：`ActionResource::Network`。
  - [ ] 明确是否需要扩展 method、headers、body、timeout。
  - [x] output schema：status、headers、body、content_type、truncated、final_url。
  - [ ] 测试：http/https、redirect、blocked host、large body、unsupported content type。

- [ ] `WebSearch`
  - [x] 当前 input schema：`query`、`max_results`、`region`、`recency`。
  - [x] permission resource：`ActionResource::Network`，execution channel 为 `web_search_backend` external capability。
  - [x] 明确 `WebSearchBackend` 的网络和权限边界。
  - [x] output schema：results、title、url、snippet、source。
  - [ ] 测试：backend missing、empty result、redaction、limit。

### 对话、任务、记忆、技能工具

- [ ] `Clarify`
  - [x] journal authority：`Clarification`。
  - [x] output schema：question、answers、selected。
  - [ ] 测试：capability missing、single answer、multi answer、cancel。

- [ ] `SendMessage`
  - [x] plan 使用 `UserMessenger` 外部能力通道。
  - [ ] 明确 parent/subagent message 行为。
  - [x] output schema：sent、target、message_id。
  - [ ] 测试：正常发送、目标不存在、interrupt。

- [ ] `Todo`
  - [x] output schema：todos、changed、merge mode。
  - [x] permission resource：当前走 `generic_action_plan()`，需要确认是否补 todo resource。
  - [ ] 测试：新增、更新、取消、非法状态、多个 in_progress。

- [ ] `memory`
  - [x] descriptor name 是 `memory`。
  - [ ] 明确 visibility、thread settings、敏感内容过滤。
  - [x] output schema：drafts、saved、visibility、thread。
  - [x] permission resource：memory mutation。
  - [ ] 测试：draft、save、reject、redaction、capability missing。

- [ ] `TaskStop`
  - [ ] 明确 stop reason、scope、是否终止当前 run。
  - [x] output schema：stopped、reason。
  - [x] permission resource：当前走 `generic_action_plan()`，需要确认是否补 run control resource。
  - [ ] 测试：正常 stop、重复 stop、无 reason。

- [ ] `skills_list`
  - [x] descriptor name 是 `skills_list`。
  - [x] output schema：skills、name、description、source。
  - [ ] 测试：无 skill、有 skill、过滤。

- [ ] `skills_view`
  - [x] descriptor name 是 `skills_view`。
  - [x] output schema：skill、content、metadata。
  - [ ] 测试：存在、不存在、路径越界。

- [ ] `skills_invoke`
  - [x] descriptor name 是 `skills_invoke`。
  - [ ] 明确是否需要 dynamic schema。
  - [x] output schema：invocation、result、events。
  - [ ] 测试：成功、skill 不存在、参数错误、脚本失败。

### MiniMax 工具

共享要求：

- [x] 执行路径通过 `ToolNetworkBrokerCap`。
- [x] descriptor 的 `required_capabilities` 当前包含 `ProviderCredentialResolver`；media / artifact 输出相关工具还会声明 `BlobWriter`。
- [x] `ToolNetworkBrokerCap` 是执行路径依赖，不在 descriptor capability 过滤中表达。
- [x] service-bound 工具还受 provider capability routes 过滤。
- [x] API key 和 token 不进入错误消息。
- [ ] input schema 对模型、尺寸、时长、format、voice、file_id 严格约束。
- [x] output schema 区分 text、image、video、audio、file、model、tokens。
- [x] provider media 下载走安全 content-type 校验。
- [x] async query 工具明确轮询和任务状态。
- [ ] 所有工具有 capability missing 测试。
- [ ] 所有工具有 provider error redaction 测试。

逐工具：

- [ ] `MiniMaxTextToImage`
- [ ] `MiniMaxImageToImage`
- [ ] `MiniMaxTextToVideo`
- [ ] `MiniMaxImageToVideo`
- [ ] `MiniMaxFirstLastFrameToVideo`
- [ ] `MiniMaxSubjectReferenceVideo`
- [ ] `MiniMaxVideoGenerationQuery`
- [ ] `MiniMaxVideoTemplate`
- [ ] `MiniMaxVideoTemplateQuery`
- [ ] `MiniMaxTextToSpeech`
- [ ] `MiniMaxTextToSpeechAsync`
- [ ] `MiniMaxTextToSpeechAsyncQuery`
- [ ] `MiniMaxVoiceClone`
- [ ] `MiniMaxVoiceDesign`
- [ ] `MiniMaxListVoices`
- [ ] `MiniMaxDeleteVoice`
- [ ] `MiniMaxLyricsGeneration`
- [ ] `MiniMaxMusicGeneration`
- [ ] `MiniMaxMusicCoverPreprocess`
- [ ] `MiniMaxFileUpload`
- [ ] `MiniMaxFileList`
- [ ] `MiniMaxFileRetrieve`
- [ ] `MiniMaxFileDelete`
- [ ] `MiniMaxModelsList`
- [ ] `MiniMaxModelRetrieve`
- [ ] `MiniMaxResponses`
- [ ] `MiniMaxResponsesInputTokens`
- [ ] `MiniMaxAnthropicMessages`
- [ ] `MiniMaxAnthropicCountTokens`
- [ ] `MiniMaxAnthropicModelsList`
- [ ] `MiniMaxAnthropicModelRetrieve`

### Seedance 工具

共享要求：

- [x] 执行路径通过 `ToolNetworkBrokerCap`。
- [x] descriptor 的 `required_capabilities` 当前包含 `ProviderCredentialResolver`；query 工具还会声明 `BlobWriter`。
- [x] `ToolNetworkBrokerCap` 是执行路径依赖，不在 descriptor capability 过滤中表达。
- [x] service-bound 工具还受 provider capability routes 过滤。
- [x] data URL 只接受安全 mime 和 base64。
- [x] output schema 区分 task id、status、video artifact/blob/url。
- [x] async query 状态机明确。
- [ ] provider error redaction 测试。

逐工具：

- [ ] `SeedanceTextToVideo`
- [ ] `SeedanceImageToVideo`
- [ ] `SeedanceVideoGenerationQuery`

### runtime 注入工具

- [x] `tool_search`
  - [x] output schema 已存在。
  - [x] result budget 使用 `BudgetMetric::Bytes`。
  - [x] input schema 仍需决定是否拒绝未知字段。
  - [x] 搜索结果解释命中字段和 materialization reason。
  - [x] backend 错误降级为结构化 `backend_failed` 输出。

- [x] `background_agent`
  - [x] input schema 已设置 `additionalProperties: false`。
  - [x] capability readiness：`jyowo.background_agent.starter`。
  - [x] output schema：thread/session id、status、title、error。
  - [x] permission mode、session snapshot、model config 行为文档。
  - [x] 测试：capability missing、启动成功、策略拒绝、输出契约。

- [ ] `agent_team`
  - [x] input schema 已设置 `additionalProperties: false`。
  - [x] capability readiness：`jyowo.agent_team.runner`。
  - [x] output schema：team id、members、status、message count、errors。
  - [ ] topology、max turns、停止语义文档。
  - [ ] 测试：capability missing、启动成功、重复启动、stop/report。

- [x] `agent`
  - [x] capability readiness：`ToolCapability::SubagentRunner`。
  - [x] output schema：subagent id、status、summary、transcript ref、usage。
  - [x] 明确 parent/subagent 权限转发和取消语义。
  - [x] input schema 已设置 `additionalProperties: false`。

- [x] team control tools：`dispatch`、`message`、`pause_worker`、`resume_worker`、`spawn_worker`、`stop_team`、`team_status`
  - [x] input schema 已按工具拆字段。
  - [x] output schema 已按工具声明。
  - [x] plan resources 已建模 `TeamControl`。
  - [x] 测试：非法 agent id。
  - [x] 测试：stop 后 dispatch 失败。
  - [x] 测试：status 成功路径。
  - [x] 测试：非法 agent id、目标不存在、重复 pause/resume、stop 后操作、status。

### MCP / plugin 生成工具

- [ ] MCP wrapper
  - [x] 保留上游 `input_schema` 和 `output_schema`。
  - [x] descriptor 使用 `ToolOrigin::Mcp(...)`。
  - [x] authorization 已有 MCP resource 建模。
  - [x] cancel/interrupt ack 行为已有实现和测试覆盖。
  - [x] 命名、注销、shadow 生命周期矩阵测试。

- [ ] plugin sidecar tool
  - [x] 使用 JSON Schema validate input。
  - [x] descriptor 使用 `ToolOrigin::Plugin { plugin_id, trust }`。
  - [x] 当前 manifest 不支持 output schema。
  - [x] trust level、destructive、capability 许可边界测试。
  - [ ] 如需 output schema，先扩展 manifest contract。

## 分阶段执行计划

### Phase 0：建立基线，防止 AI 漏工具

- [x] 文档已按当前代码重新校准工具清单。
- [x] 已有默认工具集关键名称注册测试。
- [x] 新增完整工具清单快照：默认工具集名称和数量。
- [x] 新增 feature-gated 工具清单测试：`programmatic-tool-calling`、`minimax-tools`、`seedance-tools`。
- [x] 新增 descriptor 完整性测试：所有工具必须有 name、group、budget、search_hint 策略。
- [ ] 新增 capability 映射测试：required capabilities 与 execute 中实际 capability 调用一致。
- [x] 新增文档检查：本文中的 builder 工具名必须与注册列表一致。
- [x] 新增运行时工具清单检查：SDK、MCP、plugin、subagent、team runtime 注入工具不能漏。
- [x] 新增 ToolPool session assembly 快照：capability、service route、tenant policy、profile、tool search partition。

### Phase 1：契约和 schema

- [x] 严格化 input schema。
- [x] 明确 descriptor schema 与 `Tool::validate()` 的职责边界。
- [x] 补 output schema。
- [x] 补 search_hint 或扩展 descriptor metadata。
- [x] 更新 `ToolDescriptor` contract 测试。
- [x] 增加 schema descriptor 与 validate 行为一致性测试。
- [x] 更新 UI 渲染类型。

### Phase 2：权限和资源建模

- [x] 文件、命令、网络类基础资源已有部分 `ActionResource` 建模。
- [x] 文档已审计当前 `generic_action_plan()` 使用者。
- [x] 扩展 `ActionResource` contract，覆盖 Memory / Process / TeamControl / Blob 等当前缺失资源。
- [x] 为 memory、process read/stop、runtime 注入工具补或确认 `ActionResource`。
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

### Phase 4：provider 工具完整化

- [ ] MiniMax schema / output / network / redaction 全覆盖。
- [ ] Seedance schema / output / network / redaction 全覆盖。
- [x] provider media 类型安全测试。
- [x] async query 状态测试。
- [x] provider service binding / route gating 文档和测试。

### Phase 5：runtime 注入工具完整化

- [x] `tool_search` materialization reason。
- [x] `background_agent` output schema 和权限语义。
- [x] `agent_team` output schema 和停止语义。
- [x] `agent` output schema。
- [x] `agent` 权限继承、取消语义。
- [x] team control tools input/output schema、resource 建模。
- [x] team control tools 错误行为。

### Phase 6：MCP / plugin / deferred tools

- [x] 文档已补 MCP wrapper 当前行为。
- [x] 文档已补 plugin sidecar 当前行为。
- [x] scorer 已覆盖 name parts、description、search_hint、MCP parsing、discovered penalty 等信号。
- [x] required capabilities 是否进入 scorer 规则需定案。
- [x] scorer 加权规则继续补端到端检索测试。
- [x] search backend 错误降级。
- [x] deferred delta 增加 reason。
- [x] UI 展示 deferred tools 变化。

## 开发时的固定检查命令

按改动范围选择最小命令。

Rust 工具模块：

```sh
cargo test -p jyowo-harness-tool --features builtin-toolset
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

subagent / engine runtime tools：

```sh
cargo test -p jyowo-harness-engine subagent_tool_feature
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

- [ ] 工具出现在正确 feature 的 registry 或 runtime append 路径中。
- [ ] descriptor 完整。
- [ ] input schema 严格且有测试。
- [ ] output schema 存在且有测试。
- [ ] validate 覆盖错误输入。
- [ ] plan 建模真实 resources、workspace access、network access、execution channel。
- [ ] permission review 可读。
- [ ] capability 缺失时错误明确。
- [ ] execute 只使用 `AuthorizedToolInput`。
- [ ] 网络工具不绕过 broker，或有明确例外文档和权限策略。
- [ ] 高风险工具有确认策略。
- [ ] 长任务有 timeout / heartbeat / interrupt。
- [ ] 大输出有 budget / offload。
- [ ] 敏感内容被 redaction。
- [x] UI 能正确展示 pending、running、success、failure、offload。
- [ ] 单元测试、contract test、集成测试按工具覆盖。
- [ ] 文档更新。

## 当前最高优先级 checklist

- [x] 定案 `jyowo-harness-tool` 保持 `default = []`。
- [x] 建立 builder 工具清单和 runtime 注入工具清单快照测试。
- [x] 建立 ToolPool session assembly 过滤链快照测试。
- [x] 建立默认工具集 journal authority 快照测试。
- [x] 给基础工具补 output schema。
- [x] 明确 descriptor schema 与 `Tool::validate()` 的校验职责，并补未知字段策略测试。
- [x] 扩展 `ActionResource`，覆盖 Memory / Process / TeamControl / Blob 等当前无枚举变体的资源。
- [x] 为 `memory`、`ProcessRead`、`ProcessStop`、`ReadBlob`、runtime 注入工具补或确认资源建模。
- [x] 补 provider service binding / route gating 文档和测试。
- [x] 明确 `WebSearchBackend` 的网络边界。
- [x] 给缺失的基础工具补 `search_hint` 或扩展 descriptor metadata。
- [x] 建立文档和代码清单一致性检查。
