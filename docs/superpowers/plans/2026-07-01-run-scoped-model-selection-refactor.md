# Run-Scoped Model Selection Refactor Plan

> **For agentic workers:** REQUIRED SUB-SKILL: use `superpowers:subagent-driven-development` task-by-task. Every task must end with a read-only subagent audit. No production mock/fake/noop/placeholder implementation is allowed.

**Goal:** 让新建空对话不再被默认模型固化，并允许同一会话中每一轮 run 使用不同模型。

**Core Design:** `Conversation` 是对话容器，`Run` 是一次执行，`Model` 是 run 级配置。Conversation/session identity 只表达 workspace、tenant、session scope；模型、工具配置、权限模式、runtime prompt context 都进入 run effective config。

**Branch/Doc:** 当前分支是 `main`。执行时先把本计划保存到 `docs/superpowers/plans/2026-07-01-run-scoped-model-selection-refactor.md`，直接在 `main` 上生成。保留现有未提交改动，不回滚、不覆盖无关文件。

---

## Mandatory Execution Protocol

每个 Task 都必须按这个顺序执行：

1. **Task Intent Check:** 先写明本 Task 的目标、涉及边界、禁止做的事、验收条件。
2. **Implement:** 只做本 Task 规定的修改。
3. **Local Gate:** 运行本 Task 指定测试。
4. **Diff Review:** 检查 `git diff`，确认没有无关重构、孤儿 import、生产 mock/fake/noop。
5. **Subagent Audit:** 派一个只读 subagent 审计本 Task 是否完整实现。审计输出必须是 `PASS` 或 `FAIL`，并带文件/行号证据。
6. **Fix If FAIL:** FAIL 必须修完并重新审计。
7. **Commit:** 每个 Task 单独提交。提交前确认没有 staged 无关文件。

禁止新增生产环境假实现。测试只能使用现有 test adapter、in-memory store、scripted provider；不能把 mock runtime 接进 production path。

---

## Target Architecture

新增或调整这些稳定接口：

```rust
pub struct RunModelSnapshot {
    pub model_config_id: Option<String>,
    pub provider_id: String,
    pub model_id: String,
    pub display_name: String,
    pub protocol: ModelProtocol,
    pub context_window: u32,
    pub max_output_tokens: u32,
    pub conversation_capability: ConversationModelCapability,
}
```

`RunStartedEvent` 必须包含：

```rust
pub model: RunModelSnapshot
```

`StartRunRequest` 必须包含：

```rust
pub model_config_id: String
```

`ComposerSubmitPayload` 必须包含：

```ts
modelConfigId: string
```

Session identity hash 只允许包含：

```text
workspace_ref
workspace_root
workspace_bootstrap
tenant_id
session_id
user_id
team_id
```

这些字段必须移出 session identity，成为 run-level execution config：

```text
model_id
protocol
model_extra
tool_search
tool_profile
permission_mode
interactivity
system_prompt_addendum
max_iterations
context_compression_trigger_ratio
runtime_prompt_context_hash
effective_prompt_inputs_hash
agent_options
```

旧数据策略：不迁移旧 runtime events，不加兼容分支。旧 `.jyowo/runtime/events` 下的历史 session 不保证继续可读或可运行。

---

## Task 1: Contracts And Hash Boundary

**Implement:**

- 在 `crates/jyowo-harness-contracts` 增加 `RunModelSnapshot`。
- 给 `RunStartedEvent` 增加非 optional `model: RunModelSnapshot`。
- 给 `ProviderCredentialResolveContext` 增加 `model_config_id: Option<String>`，用于 tool credential resolver 使用当前 run 的模型配置。
- 在 `crates/jyowo-harness-session` 中拆清 session identity hash：`session_options_hash` 只 hash identity 字段。
- 删除 `conversation_session_options_hash_matches` 里遍历 supported models 的兼容逻辑。打开 conversation 时只比较 identity hash。
- 更新 schema export、contract snapshots、Rust contract tests、frontend run event Zod schema。

**Tests:**

- `cargo test -p jyowo-harness-session session_options_hash`
- `cargo test -p jyowo-harness-contracts`
- `pnpm -C apps/desktop test -- run-event-schema`

**Audit prompt:** “只读审计 Task 1：确认 RunStartedEvent 强制包含模型快照，session identity hash 不再包含模型/工具/权限/run 配置，且没有旧 hash 兼容分支。”

---

## Task 2: Run-Level Execution Options In SDK

**Implement:**

- 新增 `ConversationRunOptions`，承载 run-level fields：模型、protocol、tool profile、tool search、permission mode、interactivity、context compression、model extra、max iterations、system prompt addendum、`model_config_id`。
- 修改 `ConversationTurnRequest`：保留 session identity options，新增 `run_options: ConversationRunOptions`；移除 `permission_mode_override`。
- `submit_conversation_turn` 使用 `run_options` 构造 engine、tool pool、system prompt、model snapshot、attachment modality validation。
- `RunContext`、`SessionTurnContext`、`ToolContext` 传递 `RunModelSnapshot` 和 `model_config_id`。
- `RunStartedEvent.effective_config_hash` 改成 run effective config hash，必须包含 run options、runtime prompt context hash、workspace prompt input hash。
- `SessionCreated.effective_config_hash` 不再作为 conversation 继续执行的模型一致性依据；更新 backend docs 说明。
- 更新 subagent/team/background run 调用点，让它们显式构造 `ConversationRunOptions`，不从 session identity 读取模型。

**Tests:**

- 新增 SDK 测试：同一 session 先用 DeepSeek provider/model run，再用 Minimax provider/model run，第二次不得出现 `conversation session options do not match`。
- 新增测试：改变 permission mode / tool profile / context compression 后，同一 conversation 可继续开新 run，且 `RunStarted.effective_config_hash` 不同。
- 运行：`cargo test -p jyowo-harness-sdk runtime_assembly`

**Audit prompt:** “只读审计 Task 2：确认模型和执行配置只属于 run，不属于 session identity；所有 submit_conversation_turn 调用点显式传 run_options。”

---

## Task 3: Desktop Draft Conversation Metadata

**Implement:**

- 用新的 `ConversationMetadataStore` 替代 `ConversationModelConfigStore`。
- 新 store 文件：`.jyowo/runtime/conversation-metadata.json`。
- record shape 固定为：
  ```json
  {
    "version": 1,
    "conversations": {
      "<session_id>": {
        "id": "<session_id>",
        "title": "New conversation",
        "createdAt": "<rfc3339>",
        "updatedAt": "<rfc3339>",
        "defaultModelConfigId": "<config_id or null>",
        "state": "draft|active"
      }
    }
  }
  ```
- `create_conversation` 只写 metadata，不调用 `open_or_create_conversation_session`。
- `list_conversations` 合并 metadata drafts 和 journal summaries；不得为了空列表自动创建 default runtime session。
- `get_conversation` 对 draft 返回空 `messages`、metadata title、`modelConfigId`。
- `delete_conversation` 同时删除 metadata record；active conversation 仍删除 journal session。
- 删除旧 `conversation-model-settings.json` 读取路径和旧 store trait，不做迁移。

**Tests:**

- `cargo test -p jyowo-desktop-shell create_conversation`
- 新增命令测试：新建对话后 event store 没有 `SessionCreated`。
- 新增命令测试：draft 能 list/get/delete。
- 运行：`cargo test -p jyowo-desktop-shell commands`

**Audit prompt:** “只读审计 Task 3：确认空对话不会写 runtime journal，metadata store 是唯一 draft 来源，旧 conversation model config store 已移除。”

---

## Task 4: Desktop Start Run Uses Explicit Model Config

**Implement:**

- `StartRunRequest` 增加必填 `model_config_id`。
- `start_run` 必须按 request 的 `model_config_id` 解析 provider config、API key、descriptor、provider、protocol。
- 构造 `ConversationRunOptions`，把 `model_config_id` 和模型快照传入 SDK。
- `start_run` 成功拿到 `RunStarted` 后，把 conversation metadata 更新为 `state = active`，并把 `defaultModelConfigId` 设为本次 `model_config_id`。
- `DesktopProviderCredentialResolver` 优先使用 `ProviderCredentialResolveContext.model_config_id`；只有 route-kind 明确配置时使用 capability route。
- 删除或停止暴露 `set_conversation_model_config` 作为发送前运行时依赖。前端不再调用它。
- Automations/background supervisor 必须显式选择模型：优先 conversation metadata default，其次 provider settings default；没有可用 config 直接 fail-closed。

**Tests:**

- 新增命令测试：默认 DeepSeek 新建 draft，发送时传 Minimax，RunStarted.model 是 Minimax。
- 新增命令测试：同一 active conversation 第二轮传 DeepSeek，RunStarted.model 变为 DeepSeek。
- 新增命令测试：缺失/无 key/不存在的 `model_config_id` fail-closed，且不写 active metadata。
- 运行：`cargo test -p jyowo-desktop-shell commands`

**Audit prompt:** “只读审计 Task 4：确认 start_run 的模型真相只来自 request.model_config_id，credential resolver 使用 run model_config_id，未保留 set_conversation_model_config 旧路径。”

---

## Task 5: Frontend Composer And Command Client

**Implement:**

- `apps/desktop/src/shared/tauri/commands.ts`：`startRunRequestSchema` 增加 `modelConfigId`，删除 `setConversationModelConfig` client API 和 schema。
- `ComposerSubmitPayload` 增加 `modelConfigId`。
- `ConversationWorkspace` 使用本地 `selectedModelConfigId`：
  - 初始化为 `conversation.modelConfigId ?? providerSettings.defaultConfigId ?? ''`。
  - 下拉切换只改本地 state，不调用后端。
  - submit/review continue 必须带当前 `modelConfigId`。
  - send disabled when no configured model with API key.
  - submit 成功后 invalidate conversation detail/list，让 metadata default 回显。
- 附件能力继续由当前本地选中模型的 descriptor 决定。
- UI 不新增解释性文案；错误使用现有 composer error 区域。

**Tests:**

- 更新 `Composer.test.tsx`：提交 payload 包含 `modelConfigId`。
- 更新 `ConversationWorkspace.test.tsx`：
  - 切换模型不调用 removed command。
  - startRun 收到 Minimax config。
  - submit 成功后 query invalidate。
  - 没有可用模型时 send disabled。
- 运行：`pnpm -C apps/desktop test -- ConversationWorkspace Composer commands`

**Audit prompt:** “只读审计 Task 5：确认模型选择是本地 composer state，发送 payload 必带 modelConfigId，前端没有旧 setConversationModelConfig 调用。”

---

## Task 6: Projection, Timeline, And Model Visibility

**Implement:**

- `ConversationWorktree` projection 从 `RunStarted.model` 填充 `AssistantWork.model`。
- `ConversationTimelineEvent` 的 `run.started` payload 包含安全模型快照：provider、model id、display name、protocol、modelConfigId；不得包含 API key/base URL。
- 前端 `AssistantWorkView` 在 assistant header 显示紧凑模型标签，例如 `MiniMax M3`；不要新增卡片。
- 更新 story/test fixtures 为真实 contract shape；不使用假字段。

**Tests:**

- `cargo test -p jyowo-harness-journal conversation_worktree_projector conversation_read_model`
- `pnpm -C apps/desktop test -- conversation-timeline run-event-schema`
- 检查快照不含 secret/baseUrl/apiKey。

**Audit prompt:** “只读审计 Task 6：确认每个 assistant work 都能追溯 run model，UI 只显示非敏感模型信息，projection 没有裸 JSON 或 secret 泄漏。”

---

## Task 7: Docs, Gates, And Cleanup

**Implement:**

- 更新 backend docs：conversation identity vs run execution config、draft conversation、run model snapshot、credential resolver fail-closed。
- 更新 frontend docs：composer model selection 是 per-run control，发送成功后才持久化为 conversation default。
- 删除旧测试、旧 command client API、旧 store、旧 hash 兼容测试。
- 检查 changed production files：
  ```bash
  git diff --name-only | rg '^(apps/desktop/src|apps/desktop/src-tauri/src|crates)/'
  ```
  对这些文件跑新增假实现扫描：
  ```bash
  rg -n "mock|fake|noop|placeholder|TODO|coming soon|experimental" <changed-production-files>
  ```
  新增命中一律失败，测试文件除外。

**Final Gates:**

```bash
cargo fmt --all --check
cargo check --workspace
cargo test --workspace
pnpm check:docs
pnpm check:desktop
pnpm check:rust
pnpm check
```

**Audit prompt:** “只读审计 Task 7：确认 docs 与实现一致，旧兼容路径已删除，所有指定门禁通过，生产文件无新增 fake/mock/noop/placeholder。”

---

## Acceptance Criteria

- 新建对话不会写 `SessionCreated`。
- 默认 DeepSeek 的空对话，选择 Minimax 后首次发送成功。
- 同一 conversation 可连续使用不同 provider/model 发送多轮。
- `SessionCreated.options_hash` 不再因为模型、权限、tool profile、压缩参数变化而阻断新 run。
- `RunStarted.model` 记录非敏感模型快照。
- 读历史 conversation 不依赖当前 provider harness。
- 没有旧 hash 兼容分支。
- 没有生产 mock/fake/noop。
- 每个 Task 都有 subagent PASS 审计记录。
