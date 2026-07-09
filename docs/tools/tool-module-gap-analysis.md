# 工具模块欠缺文档

本文用于指导后续工具模块开发与完善。

项目当前采用 AI 辅助开发。本文刻意写得较细，目标是让执行者不要漏掉工具、文件、契约、权限、schema、测试和宿主集成点。

## 适用范围

本文覆盖当前已梳理到的工具相关模块：

- `crates/jyowo-harness-contracts/src/tool.rs`
- `crates/jyowo-harness-contracts/src/deferred_tools.rs`
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
- `crates/jyowo-harness-sdk/src/harness/tool_pool.rs`
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

shadow 规则已经存在：

- builtin 与 builtin 重名时保留已有。
- plugin、MCP、builtin 的优先级需要以后补完整文档和测试矩阵。

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
- `Memory` -> `crates/jyowo-harness-tool/src/builtin/memory.rs`
- `TaskStop` -> `crates/jyowo-harness-tool/src/builtin/task_stop.rs`
- `SkillsList` -> `crates/jyowo-harness-tool/src/builtin/skills.rs`
- `SkillsView` -> `crates/jyowo-harness-tool/src/builtin/skills.rs`
- `SkillsInvoke` -> `crates/jyowo-harness-tool/src/builtin/skills.rs`

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

- deferred tools delta attachment。
- tool search backend。
- policy / scorer / coalescer 测试。
- `force defer` 在 tool search disabled 时应失败的测试。

## 能力依赖清单

工具注册成功不代表能执行成功。很多工具依赖 `ToolContext.cap_registry` 中的 capability。

| Capability | 影响工具 | 需要补齐的开发工作 |
|---|---|---|
| `BrokeredPlatformRuntimeCap` | `Worktree`、`Session`、`Artifact`、`BrowserUse`、`ComputerUse`、`ImageGeneration`、`NotebookEdit`、`LSP`、`Automation`、`Workflow` | 宿主 runtime 必须实现并注册。工具列表展示前应能提示 capability 是否可用。 |
| `ToolNetworkBrokerCap` | `WebFetch`、MiniMax、Seedance | 所有外部网络访问必须通过 broker。需要持续用边界测试防回归。 |
| `WebSearchBackend` | `WebSearch` | 需要确认 backend 是否执行等价的网络权限、redaction、审计。 |
| `DiagnosticsRunnerCap` | `Diagnostics` | 需要确认 runner 的工作目录、命令来源、输出清洗、超时策略。 |
| `ClarifyChannelCap` | `Clarify` | 需要确认 UI 通道存在时才展示工具，或执行前给出清晰错误。 |
| `MemoryToolRuntimeCap` | `Memory` | 需要确认记忆写入、可见性、线程设置、敏感内容过滤。 |
| sandbox backend | `Bash`、`ProcessStart`、`ExecuteCode`、部分文件/Git 操作 | 需要确认 sandbox policy、workspace scope、超时、interrupt 均生效。 |
| blob store | 大输出、offload、`ReadBlob` | 需要确认 offload 后可读、权限、retention、redaction。 |

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
- [ ] 新增测试：默认 feature 下 builder 会注册基础内置工具。
- [ ] 新增测试：`default-features = false` 时 builder 行为符合预期。
- [ ] 新增测试：启用 `builtin-toolset` 时默认工具数量和关键工具名稳定。
- [ ] 检查下游 crate 是否仍需要 `default-features = false`。
- [ ] 如果下游保留 `default-features = false`，确认它们都显式声明了需要的工具 feature。

### G2. brokered platform 工具不是完整实现，只是宿主转发层

相关文件：

- `crates/jyowo-harness-tool/src/builtin/brokered_platform.rs`

当前状态：

- `BrokeredPlatformTool` 统一实现 validate / plan / execute。
- `execute_authorized()` 把 `BrokeredPlatformRuntimeRequest` 交给宿主 runtime。
- 工具 crate 内没有真实执行 worktree、browser、computer、image、notebook、LSP、automation、workflow。

影响：

- 工具注册成功不代表可用。
- capability 缺失时执行失败。
- 工具 schema 和权限 plan 很难覆盖每个具体 action。

处理方向：

- [ ] 在工具列表或 descriptor metadata 中标记 brokered 工具需要宿主 capability。
- [ ] UI 层展示 capability missing 的可操作错误，而不是普通失败。
- [ ] SDK 层提供 capability readiness 查询。
- [ ] 为每个 brokered 工具补宿主 runtime contract 文档。
- [ ] 为每个 brokered 工具补 host-side integration test 或 mock runtime test。

### G3. brokered platform 工具 input schema 太宽

当前例子：

- `Worktree` 只有 `action` required，`action` 是自由字符串。
- `Session` 只有 `action` required，字段依赖 action 但 schema 不表达。
- `BrowserUse`、`ComputerUse`、`Automation` 同样依赖自由字符串 action。
- `Workflow` required 是 `name`，但 `params` 没有按 workflow schema 约束。

影响：

- 模型容易传错 action。
- validate 只检查 object，不能早发现错误。
- permission review 只能按整个 tool invocation 展示，很难细化到资源。
- 宿主 runtime 承担过多校验责任。

处理方向：

- [ ] 为每个 brokered 工具定义 action enum。
- [ ] 使用 `oneOf` / `anyOf` 表达不同 action 的 required 字段。
- [ ] 为高风险 action 增加确认文案。
- [ ] 为不同 action 生成不同 `ActionResource`。
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

- [ ] 给 `ToolDescriptor` 增加或填充 output schema。
- [ ] 对结构化输出建立 Rust 类型。
- [ ] 对每个 output schema 增加 contract test。
- [ ] UI 只依赖 schema 中声明的字段。
- [ ] 对 Mixed / Blob / Offloaded 输出写清楚 schema 表达方式。

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

影响：

- 未知字段可能被接受。
- 模型输入拼错字段时不一定失败。
- permission plan 可能没有覆盖未知字段里的意图。

处理方向：

- [ ] 决定默认严格还是按工具定制。
- [ ] 如果默认严格，在 `object_schema()` 中加入 `additionalProperties: false`。
- [ ] 如果个别工具需要透传对象，显式允许并在文档中说明。
- [ ] 所有工具补“未知字段被拒绝 / 被允许”的测试。
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

部分工具会自定义 plan，但需要逐个确认。

影响：

- 权限 UI 可能看不到具体文件、命令、网络 host、MCP 资源。
- persisted decision scope 可能过宽。
- action plan hash 可以防篡改，但如果资源本身没建模，审计粒度仍不足。

处理方向：

- [ ] 为每个文件工具建模 `ActionResource::FileRead` / `FileWrite` / `FileDelete`。
- [ ] 为 `Bash`、`ProcessStart` 建模 `ActionResource::Command`。
- [ ] 为 `WebFetch`、MiniMax、Seedance 建模 `ActionResource::Network`。
- [ ] 为 brokered platform 高风险 action 建模对应资源。
- [ ] 为 Git 写操作建模 command 或 workspace mutation。
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
- 普通内置工具大多通过 helper 得到默认 metadata。
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

- [ ] 明确 `WebSearchBackend` 是否必须通过 network broker。
- [ ] 如果必须，调整 trait 或 runtime 实现。
- [ ] 如果不必须，补安全说明和单独 permission policy。
- [ ] 扩展 `check-tool-network-broker-boundary` 覆盖 WebSearch backend。
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

- [ ] 为 `Bash` 定义 timeout 和 heartbeat 策略。
- [ ] 为 `ProcessStart` / `ProcessRead` / `ProcessStop` 定义 timeout 和 heartbeat 策略。
- [ ] 为 `ExecuteCode` 定义 timeout 和 sandbox interrupt 策略。
- [ ] 为 MiniMax 视频、音乐、TTS async 工具定义长任务策略。
- [ ] 为 Seedance 视频任务定义长任务策略。
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

欠缺点：

- 文件读、grep、git diff、web fetch、process output 的预算不应完全相同。
- 二进制、图片、音频、视频、blob 输出需要更明确的预算表达。
- provider 工具的 artifact 输出要明确是 inline、blob、URL 还是 mixed。

处理方向：

- [ ] 给文本型大输出工具单独设置预算。
- [ ] 给二进制 / provider media 工具设置 blob-first 策略。
- [ ] 给 process output 设置 stdout/stderr 分离预算。
- [ ] 给 Git diff 设置 patch-aware preview。
- [ ] 给 WebFetch 设置 content-type aware budget。
- [ ] 增加 offload 之后 `ReadBlob` 可恢复读取的集成测试。

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
- [ ] 给动态 schema 加 contract test。
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

处理方向：

- [ ] 所有工具补 metadata。
- [ ] scorer 使用 descriptor metadata、group、required capabilities、risk level。
- [ ] 搜索结果解释包含命中字段。
- [ ] deferred delta 中加入可读 reason。
- [ ] 增加“文件任务命中文件工具”、“Git 任务命中 Git 工具”、“图片任务命中图片工具”的检索测试。

### G13. 插件和 MCP 工具生命周期需要补完整文档和测试

相关文件：

- `crates/jyowo-harness-tool/src/registry.rs`
- `crates/jyowo-harness-contracts/src/tool.rs`

当前已有：

- `register_from_plugin()`。
- `deregister_from_plugin()`。
- `deregister_mcp_tool()`。
- MCP canonical name 相关函数。
- shadowed registration 记录。

欠缺点：

- 插件工具 trust level 与 capability 许可边界需要更完整测试。
- MCP 工具命名、注销、shadow 行为需要完整矩阵。
- shadowed 记录是否需要暴露给 UI / telemetry 未明确。

处理方向：

- [ ] 写插件工具注册生命周期文档。
- [ ] 写 MCP 工具注册生命周期文档。
- [ ] 补 builtin / plugin / MCP 重名优先级测试。
- [ ] 补 trust level 与 destructive 工具测试。
- [ ] 补 capability not permitted 测试。
- [ ] 决定 shadowed 是否进入事件流。

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

- [ ] UI 渲染基于 output schema 和 ToolResult 类型。
- [ ] pending permission 展示 resources、scope、severity、confirmation。
- [ ] capability missing 展示“宿主能力未注册”。
- [ ] brokered 工具展示真实 action，而不是只展示工具名。
- [ ] deferred tools 展示新增/移除和原因。
- [ ] 为每种 ToolResult 增加 UI 测试。

### G15. 测试矩阵需要从“有测试”升级为“按工具覆盖”

当前已有测试文件包括：

- `crates/jyowo-harness-tool/tests/api_contract.rs`
- `crates/jyowo-harness-tool/tests/builtin_diagnostics.rs`
- `crates/jyowo-harness-tool/tests/builtin_process_monitor.rs`
- `crates/jyowo-harness-tool/tests/builtin_skills.rs`
- `crates/jyowo-harness-tool/tests/builtin_tools.rs`
- `crates/jyowo-harness-tool/tests/capabilities.rs`
- `crates/jyowo-harness-tool/tests/contract.rs`
- `crates/jyowo-harness-tool/tests/execute_code.rs`
- `crates/jyowo-harness-tool/tests/memory_tool.rs`
- `crates/jyowo-harness-tool/tests/minimax_tools.rs`
- `crates/jyowo-harness-tool/tests/orchestrator.rs`
- `crates/jyowo-harness-tool/tests/registry.rs`
- `crates/jyowo-harness-tool/tests/registry_pool.rs`
- `crates/jyowo-harness-tool/tests/result_budget.rs`
- `crates/jyowo-harness-tool/tests/seedance_tools.rs`
- `crates/jyowo-harness-tool/tests/skill_script.rs`
- `crates/jyowo-harness-tool-search/tests/*.rs`
- `crates/jyowo-harness-sdk/tests/*tool*`

需要补的测试维度：

- [ ] descriptor contract。
- [ ] input schema strictness。
- [ ] output schema contract。
- [ ] validation error。
- [ ] permission plan resources。
- [ ] ticket mismatch 防护。
- [ ] capability missing。
- [ ] successful execution。
- [ ] redaction。
- [ ] result budget/offload。
- [ ] interrupt/timeout。
- [ ] UI rendering。

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
  - [ ] 确认路径必须在 workspace scope 内。
  - [ ] 输出 schema：content、path、size、truncated/offloaded、encoding。
  - [ ] permission resource：`ActionResource::FileRead`。
  - [ ] 测试：正常读、缺文件、目录、越界路径、大文件 offload、redaction。

- [ ] `FileEdit`
  - [ ] 确认 patch / old-new 字符串语义。
  - [ ] 输出 schema：path、changed、diff、old_hash/new_hash。
  - [ ] permission resource：`ActionResource::FileWrite`。
  - [ ] 测试：唯一匹配、多重匹配、无匹配、越界路径、并发修改。

- [ ] `FileWrite`
  - [ ] 明确覆盖和创建行为。
  - [ ] 输出 schema：path、bytes_written、content_hash。
  - [ ] permission resource：`ActionResource::FileWrite`。
  - [ ] 测试：创建、覆盖、父目录不存在、越界路径、内容 hash。

- [ ] `ListDir`
  - [ ] 输出 schema：entries、entry_type、size、modified。
  - [ ] permission resource：目录读。
  - [ ] 测试：空目录、大目录、隐藏文件、越界路径、排序。

- [ ] `Grep`
  - [ ] 明确 regex 语法、大小写、glob、limit。
  - [ ] 输出 schema：matches、file、line、column、preview、truncated。
  - [ ] permission resource：读取匹配范围。
  - [ ] 测试：无匹配、多匹配、非法 regex、大输出 offload。

- [ ] `Glob`
  - [ ] 明确 glob 语法、忽略规则、排序。
  - [ ] 输出 schema：paths、truncated。
  - [ ] permission resource：workspace read。
  - [ ] 测试：递归、ignore、空结果、大结果。

- [ ] `ReadBlob`
  - [ ] 明确 blob id、retention、权限。
  - [ ] 输出 schema：blob_id、content、content_type、size。
  - [ ] 测试：存在、不存在、过期、权限、二进制。

### Git 工具

- [ ] `GitStatus`
  - [ ] 输出 schema：branch、ahead/behind、changed files、untracked。
  - [ ] permission resource：workspace read。
  - [ ] 测试：干净仓库、dirty、untracked、非 git 目录。

- [ ] `GitDiff`
  - [ ] 输出 schema：files、patch、stats、truncated/offloaded。
  - [ ] permission resource：workspace read。
  - [ ] 测试：staged、unstaged、path filter、大 diff。

- [ ] `GitShow`
  - [ ] 输出 schema：commit、files、patch/content。
  - [ ] permission resource：workspace read。
  - [ ] 测试：commit、path、非法 rev。

- [ ] `GitLog`
  - [ ] 输出 schema：commits、hash、author、date、subject。
  - [ ] permission resource：workspace read。
  - [ ] 测试：limit、path filter、空历史。

- [ ] `GitStage`
  - [ ] 明确禁止 stage secrets 的策略是否在工具层处理。
  - [ ] 输出 schema：staged_paths、status_after。
  - [ ] permission resource：command 或 workspace mutation。
  - [ ] 测试：单文件、多文件、不存在文件、越界路径。

- [ ] `GitCommit`
  - [ ] 明确 commit message 规则、hook 行为、失败回滚。
  - [ ] 输出 schema：commit_hash、summary、status_after。
  - [ ] permission resource：command 或 workspace mutation。
  - [ ] 测试：无 staged、hook fail、成功 commit、author 配置缺失。

- [ ] `GitBranch`
  - [ ] 明确 list/create/delete/switch 行为。
  - [ ] 输出 schema：branch、action、branches。
  - [ ] permission resource：workspace mutation when write action。
  - [ ] 测试：创建、切换、删除保护、dirty worktree。

- [ ] `GitPull`
  - [ ] network access 必须明确。
  - [ ] 输出 schema：summary、changed files、conflicts。
  - [ ] permission resource：command + network。
  - [ ] 测试：up-to-date、conflict、auth fail、network fail。

- [ ] `GitPush`
  - [ ] network access 必须明确。
  - [ ] 明确禁止 force push 的策略。
  - [ ] 输出 schema：remote、branch、pushed。
  - [ ] permission resource：command + network。
  - [ ] 测试：normal push、auth fail、rejected、force push blocked。

### 执行、诊断、进程工具

- [ ] `Bash`
  - [ ] input schema 严格表达 command、cwd、env、timeout。
  - [ ] permission resource：`ActionResource::Command`。
  - [ ] sandbox policy 明确。
  - [ ] output schema：exit_code、stdout、stderr、timed_out、truncated。
  - [ ] 测试：成功、非零退出、timeout、interrupt、redaction、cwd 越界。

- [ ] `Diagnostics`
  - [ ] runner capability 缺失错误可读。
  - [ ] output schema：diagnostics、path、line、severity、source。
  - [ ] permission resource：workspace read / command。
  - [ ] 测试：TypeScript、Cargo、空诊断、解析失败。

- [ ] `ProcessStart`
  - [ ] 明确前台/后台、terminal id、cwd、env、timeout。
  - [ ] permission resource：`ActionResource::Command`。
  - [ ] output schema：process_id、started、stdout_preview、stderr_preview。
  - [ ] 测试：成功启动、启动失败、重复启动、redaction。

- [ ] `ProcessRead`
  - [ ] output schema：stdout、stderr、running、exit_code、offset。
  - [ ] 测试：运行中、已退出、未知进程、大输出、redaction。

- [ ] `ProcessStop`
  - [ ] 明确 graceful/kill 策略。
  - [ ] output schema：stopped、signal、exit_code。
  - [ ] 测试：停止运行中进程、停止未知进程、重复停止。

- [ ] `ExecuteCode`
  - [ ] 确认只在 `programmatic-tool-calling` 下出现。
  - [ ] sandbox code runtime 权限明确。
  - [ ] output schema：result、stdout、stderr、artifacts、timeout。
  - [ ] 测试：成功、异常、timeout、资源限制、危险代码隔离。

### 网络工具

- [ ] `WebFetch`
  - [ ] 所有请求必须通过 `ToolNetworkBrokerCap`。
  - [ ] input schema：url、method、headers、body、timeout、max_bytes。
  - [ ] output schema：status、headers、body、content_type、truncated、final_url。
  - [ ] permission resource：`ActionResource::Network`。
  - [ ] 测试：http/https、redirect、blocked host、large body、unsupported content type。

- [ ] `WebSearch`
  - [ ] 明确 `WebSearchBackend` 的网络和权限边界。
  - [ ] input schema：query、limit、locale、freshness。
  - [ ] output schema：results、title、url、snippet、source。
  - [ ] permission resource：network 或 backend-specific external capability。
  - [ ] 测试：backend missing、empty result、redaction、limit。

### 对话、任务、记忆、技能工具

- [ ] `Clarify`
  - [ ] output schema：question、answers、selected。
  - [ ] permission channel：clarification authority。
  - [ ] 测试：capability missing、single answer、multi answer、cancel。

- [ ] `SendMessage`
  - [ ] 明确 parent/subagent message 行为。
  - [ ] output schema：sent、target、message_id。
  - [ ] 测试：正常发送、目标不存在、interrupt。

- [ ] `Todo`
  - [ ] output schema：todos、changed、merge mode。
  - [ ] 测试：新增、更新、取消、非法状态、多个 in_progress。

- [ ] `Memory`
  - [ ] 明确 visibility、thread settings、敏感内容过滤。
  - [ ] output schema：drafts、saved、visibility、thread。
  - [ ] permission resource：memory mutation。
  - [ ] 测试：draft、save、reject、redaction、capability missing。

- [ ] `TaskStop`
  - [ ] 明确 stop reason、scope、是否终止当前 run。
  - [ ] output schema：stopped、reason。
  - [ ] 测试：正常 stop、重复 stop、无 reason。

- [ ] `SkillsList`
  - [ ] output schema：skills、name、description、source。
  - [ ] 测试：无 skill、有 skill、过滤。

- [ ] `SkillsView`
  - [ ] output schema：skill、content、metadata。
  - [ ] 测试：存在、不存在、路径越界。

- [ ] `SkillsInvoke`
  - [ ] 明确是否需要 dynamic schema。
  - [ ] output schema：invocation、result、events。
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
  - [ ] action enum：read/update/insert/delete/run。
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
- [ ] API key 和 token 不进入错误消息。
- [ ] input schema 对模型、尺寸、时长、format、voice、file_id 严格约束。
- [ ] output schema 区分 text、image、video、audio、file、model、tokens。
- [ ] provider media 下载走安全 content-type 校验。
- [ ] async query 工具明确轮询和任务状态。
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
- [ ] data URL 只接受安全 mime 和 base64。
- [ ] output schema 区分 task id、status、video artifact/blob/url。
- [ ] async query 状态机明确。
- [ ] provider error redaction 测试。

逐工具：

- [ ] `SeedanceTextToVideo`
- [ ] `SeedanceImageToVideo`
- [ ] `SeedanceVideoGenerationQueryTool`

## 分阶段执行计划

### Phase 0：建立基线，防止 AI 漏工具

- [ ] 新增工具清单测试：默认工具集名称快照。
- [ ] 新增 feature-gated 工具清单测试：`programmatic-tool-calling`、`minimax-tools`、`seedance-tools`。
- [ ] 新增 descriptor 完整性测试：所有工具必须有 name、group、budget、risk metadata。
- [ ] 新增 capability 映射测试：required capabilities 与 execute 中实际 capability 调用一致。
- [ ] 新增文档检查：本文中的工具名必须与 builder 注册列表一致。

### Phase 1：契约和 schema

- [ ] 严格化 input schema。
- [ ] 补 output schema。
- [ ] 补 metadata 和 search_hint。
- [ ] 更新 `ToolDescriptor` contract 测试。
- [ ] 更新 UI 渲染类型。

### Phase 2：权限和资源建模

- [ ] 审计所有 `plan()`。
- [ ] 为文件、命令、网络、brokered action 补 `ActionResource`。
- [ ] 补 permission review details。
- [ ] 补 persisted decision scope 测试。
- [ ] 补 ticket mismatch 和 hash mismatch 测试。

### Phase 3：执行可靠性

- [ ] 补 long-running policy。
- [ ] 补 timeout。
- [ ] 补 interrupt。
- [ ] 补 result budget。
- [ ] 补 offload / blob 恢复测试。
- [ ] 补 capability missing UX。

### Phase 4：brokered platform 完整化

- [ ] 拆 action enum。
- [ ] 拆每个 action 的 schema。
- [ ] 定义 host runtime contract。
- [ ] 补 host-side mock tests。
- [ ] UI 展示 brokered action 和 risk。

### Phase 5：provider 工具完整化

- [ ] MiniMax schema / output / network / redaction 全覆盖。
- [ ] Seedance schema / output / network / redaction 全覆盖。
- [ ] provider media 类型安全测试。
- [ ] async query 状态测试。

### Phase 6：tool search 和 deferred tools

- [ ] 所有工具 metadata 可用于检索。
- [ ] scorer 加权规则可测试。
- [ ] search backend 错误降级。
- [ ] deferred delta 增加 reason。
- [ ] UI 展示 deferred tools 变化。

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
- [ ] UI 能正确展示 pending、running、success、failure、offload。
- [ ] 单元测试、contract test、集成测试按工具覆盖。
- [ ] 文档更新。

## 当前最高优先级 checklist

- [ ] 明确 `BuiltinToolset::Default` 在无 `builtin-toolset` feature 时的行为。
- [ ] 给基础工具补 output schema。
- [ ] 给 `object_schema()` 或每个工具 validate 增加未知字段策略。
- [ ] 审计所有 `plan()` 的 `ActionResource`。
- [ ] 给 brokered platform 工具拆 action schema。
- [ ] 明确 `WebSearchBackend` 的网络边界。
- [ ] 给所有工具补 metadata / search_hint。
- [ ] 建立工具清单快照测试，确保后续不会漏工具。
