# Conversation Turn Worktree Timeline Refactor Audit

审计日期：2026-06-25

审计对象：

- `docs/plans/2026-06-25-conversation-turn-worktree-timeline-refactor.md`
- 当前仓库实现、测试、文档和漂移检查结果

结论：

该计划的主链路已经落地：Rust 合同存在，Tauri/SDK/frontend command client 已接通，`ConversationCanvas` 当前从 `pageConversationWorktree` 读取 `ConversationTurn[]`，timeline 目录内不再直接导入 `RunEvent`。

但计划没有完成。主要缺口有四类：

- Rust projector 没有生成 artifact、review request、clarification request、notice segment。
- worktree paging 没有物化表，当前使用完整 timeline 重放后按 turn 切页。这个符合计划允许的 fallback，但不是计划默认方案。
- 前端仍有 `blocks`、`pendingPermissionBlocks`、`ConversationBlockRow` 这类旧 block 命名兼容残留。
- Storybook、docs、节流压力测试、最终全量 gate 证据不完整。

## 复审补充

复审日期：2026-06-25

复审结论：既有审计主结论成立。没有发现需要推翻的任务状态判断。

需要补充三个边界判断：

- Rust projector 是当前最高风险缺口。`crates/jyowo-harness-journal/src/conversation_worktree_projector.rs` 的事件分支只覆盖 text、thinking、tool、permission、run、engine failure。`artifact`、`reviewRequest`、`clarificationRequest`、`notice` 只在合同、UI 和 segment 重排逻辑里存在，不会从真实 journal events 生成。
- 没有 worktree 物化表不等于完全违反计划。`SqliteConversationReadModelStore::page_worktree` 当前从 session start 读取完整 timeline，投影成完整 worktree 后再按 turn 切页。这符合计划允许的 fallback。仍需明确的是：它不是计划默认方案，性能和 gap 行为都不能按物化表方案验收。
- 前端旧 block 命名残留需要和 raw event 回流分开判断。`blocks`、`pendingPermissionBlocks`、`ConversationBlockRow` 违反计划里的命名和 compatibility shim 清理要求；但复审未发现 timeline 产品模型直接导入 `RunEvent`，也未发现 `get_conversation.messages` 驱动主 canvas。

复审补跑 focused tests，全部通过：

```bash
cargo test -p jyowo-harness-contracts conversation_worktree --test m1_contracts
# 2 passed

cargo test -p jyowo-harness-contracts schema_export --test m1_contracts
# 1 passed

cargo test -p jyowo-harness-journal --test conversation_worktree_projector
# 4 passed

cargo test -p jyowo-harness-journal --features sqlite --test conversation_read_model sqlite_conversation_read_model_projects_worktree
# 1 passed

cargo test -p jyowo-harness-sdk --features testing conversation_read_model_facade_returns_worktree_page --test conversation_read_model
# 1 passed

cargo test -p jyowo-desktop-shell --test commands page_conversation_worktree
# 1 passed

pnpm -C apps/desktop test src/features/conversation/conversation-production-boundaries.test.ts
# 1 file passed, 2 tests passed

pnpm -C apps/desktop test src/shared/tauri/commands.test.ts src/features/conversation/timeline/conversation-timeline-selectors.test.ts src/features/conversation/timeline/conversation-timeline-store.test.ts src/features/conversation/timeline/conversation-timeline-source.test.ts src/features/conversation/timeline/use-conversation-event-stream.test.ts src/features/conversation/timeline/conversation-timeline.test.tsx
# 6 files passed, 67 tests passed

pnpm -C apps/desktop test src/features/conversation/ConversationWorkspace.test.tsx src/features/conversation/timeline/conversation-timeline.test.tsx
# 2 files passed, 13 tests passed
```

复审未跑 full gates。因此 Task 14 仍不能判定完成。

## 任务状态总表

| Task | 状态 | 判断 |
|---|---|---|
| Task 1: Add Projection Contract Types | 已完成 | 合同类型和 schema export 已存在，focused contract tests 通过。 |
| Task 2: Build Pure Rust Worktree Projector | 部分完成 | 核心 text/tool/permission/thinking 投影存在；artifact/review/clarification/notice 未投影。 |
| Task 3: Expose Worktree Paging From Journal And SDK | 部分完成 | Journal API 和 SDK facade 存在；完整重放 fallback 存在；物化表不存在；gap 行为测试不足。 |
| Task 4: Add Tauri Command Boundary | 部分完成 | Tauri command 已注册并返回合同类型；缺 parity test 和 malformed conversation id 专项测试。 |
| Task 5: Add Frontend Zod Schema And Command Client API | 已完成 | Zod schema、default client、mock client、IPC 测试存在。 |
| Task 6: Replace Timeline Domain Model With Turn Work Tree | 部分完成 | 实际数据模型已是 `ConversationTurn[]`；仍保留 `blocks` 命名 alias。 |
| Task 7: Replace Event Reducer With Projection Store | 部分完成 | store 从 worktree hydrate，raw events 只触发 refetch；缺 100 batch 节流 IPC 压力测试。 |
| Task 8: Build Turn Work Tree Components | 已完成 | turn/work/segment 组件存在，UI 测试覆盖核心嵌套结构。 |
| Task 9: Localize Conversation Work Tree UI | 部分完成 | 中英本地化可用；键名与计划要求不一致，禁止英文断言覆盖不足。 |
| Task 10: Update Storybook State Matrix | 部分完成 | stories 已改为 worktree fixture；计划要求的状态矩阵缺项，未见 Storybook build 证据。 |
| Task 11: Remove Obsolete Event-Block Code | 部分完成 | 旧 reducer/render 文件已删除；仍有 block 命名兼容层。 |
| Task 12: Update Product And Engineering Docs | 部分完成 | 文档大多已更新；`frontend-engineering.md` 仍有旧 `ConversationBlock[]` / reducer 描述。 |
| Task 13: End-To-End Regression Tests | 部分完成 | 前端和 Tauri 回归覆盖核心问题；完整 MiniMax 截图事件流没有完整 UI fixture。 |
| Task 14: Full Gates | 未完成 | 未发现全量 gate 通过证据；本次只跑了 focused tests 和漂移检查。 |

## 逐项审计

### Task 1: Add Projection Contract Types

状态：已完成。

证据：

- `ConversationWorktreePage`、`ConversationTurnCursor`、`ConversationTurn`、`AssistantWork`、`AssistantSegment`、`ToolAttempt`、`ToolPermissionState`、`ConversationEventRef` 存在于 `crates/jyowo-harness-contracts/src/conversation.rs`。
- schema export 包含 worktree 相关 schema，位置在 `crates/jyowo-harness-contracts/src/schema_export.rs`。
- 合同测试覆盖稳定 wire shape、thinking status、schema export，位置在 `crates/jyowo-harness-contracts/tests/m1_contracts.rs`。

本次验证：

```bash
cargo test -p jyowo-harness-contracts conversation_worktree --test m1_contracts
# 2 passed

cargo test -p jyowo-harness-contracts schema_export --test m1_contracts
# 1 passed
```

### Task 2: Build Pure Rust Worktree Projector

状态：部分完成。

已完成：

- `crates/jyowo-harness-journal/src/conversation_worktree_projector.rs` 存在。
- `project_conversation_worktree_snapshot` 是纯函数。
- 已处理：
  - `user.message.appended`
  - `assistant.completed`
  - `assistant.delta`
  - `assistant.thinking.delta`
  - `tool.requested`
  - `tool.completed`
  - `tool.failed`
  - `tool.denied`
  - `permission.requested`
  - `permission.resolved`
  - `run.ended`
  - `engine.failed`
- `safe_tool_failure_summary` 固定返回安全文案。
- `safe_thinking_display` 不读取 raw chain-of-thought。
- 重复 event id 会被去重。

缺口：

- projector 没有 artifact/review/clarification/notice 的事件处理分支。
- `ArtifactSegment`、`ReviewRequestSegment`、`ClarificationRequestSegment`、`NoticeSegment` 只出现在合同和 renumber 逻辑里，不会由 projector 产出。
- projector test 只有 4 个用例，少于计划要求的覆盖面。

本次验证：

```bash
cargo test -p jyowo-harness-journal --test conversation_worktree_projector
# 4 passed
```

风险：

合同和 UI 都支持 artifact/review/clarification，但 Rust 投影目前不会生成这些 segment。真实事件到 canvas 会丢失这些产品节点。

### Task 3: Expose Worktree Paging From Journal And SDK

状态：部分完成。

已完成：

- `SqliteConversationReadModelStore::page_worktree` 存在。
- `Harness::page_conversation_worktree` 存在。
- paging 读取完整 timeline，调用 `project_conversation_worktree_snapshot`，再按 turn 切页。
- `ConversationWorktreePage` 返回 `pageCursor`、`eventCursor`、`hasMoreBefore`、`hasMoreAfter`。

缺口：

- 没有 `conversation_worktree_turn`、`conversation_worktree_assistant_segment`、`conversation_worktree_tool_attempt`、`conversation_worktree_event_ref` 物化表。
- 当前走计划允许的 fallback：从 session 完整 timeline 重放后切 turn。
- `gap` 当前固定为 `false`，未看到与 raw timeline paging 一致的 gap 行为实现。
- 不带 feature 的命令会跑 0 个相关测试，需要带实际 feature 才有覆盖。

本次验证：

```bash
cargo test -p jyowo-harness-journal --features sqlite --test conversation_read_model sqlite_conversation_read_model_projects_worktree
# 1 passed

cargo test -p jyowo-harness-sdk --features testing conversation_read_model_facade_returns_worktree_page --test conversation_read_model
# 1 passed
```

附加观察：

```bash
cargo test -p jyowo-harness-journal sqlite_conversation_read_model_projects_worktree
```

这条不指定 `--test` 和 `--features sqlite` 时会触发无关 test target 编译，并在 `crates/jyowo-harness-journal/tests/contract.rs` 因 `InMemoryEventStore` 未启用而失败。不能用它证明本计划相关测试失败。

### Task 4: Add Tauri Command Boundary

状态：部分完成。

已完成：

- `page_conversation_worktree` Tauri command 已存在。
- command 返回 `harness_contracts::ConversationWorktreePage`。
- command helper 校验非空 conversation id，解析 session id，调用 SDK facade。
- 已注册到 `generate_handler!`。
- Tauri command test 覆盖安全 worktree、空 assistant body 不显示、permission 嵌套、safe failure summary。

缺口：

- 没看到针对 `page_conversation_worktree` 的 schema parity test。
- 没看到针对 malformed conversation id 的专项 fail-closed test。

本次验证：

```bash
cargo test -p jyowo-desktop-shell --test commands page_conversation_worktree
# 1 passed
```

### Task 5: Add Frontend Zod Schema And Command Client API

状态：已完成。

证据：

- `pageConversationWorktreeRequestSchema` 和 `pageConversationWorktreeResponseSchema` 存在于 `apps/desktop/src/shared/tauri/commands.ts`。
- `CommandClient.pageConversationWorktree` 存在，并调用 `page_conversation_worktree`。
- `default-client.ts` 和 `mock-client.ts` 已接入。
- 测试覆盖 valid page、缺字段、raw `RunEvent` shape、私有路径拒绝。

本次验证包含在：

```bash
pnpm -C apps/desktop test src/shared/tauri/commands.test.ts src/features/conversation/timeline/conversation-timeline-selectors.test.ts src/features/conversation/timeline/conversation-timeline-store.test.ts src/features/conversation/timeline/conversation-timeline-source.test.ts src/features/conversation/timeline/conversation-timeline.test.tsx
# 6 files passed, 67 tests passed
```

### Task 6: Replace Timeline Domain Model With Turn Work Tree

状态：部分完成。

已完成：

- selector 返回 `ConversationTurn[]`。
- pending permission 从 nested tool attempt 读取。
- scroll anchor 使用 turn id。
- `rg -n "RunEvent" apps/desktop/src/features/conversation/timeline` 无命中。

缺口：

- `ConversationTimeline` 仍接受 `blocks?: ConversationTurn[]` alias。
- `useConversationTimeline` 仍返回 `blocks` 和 `pendingPermissionBlocks`。
- `ConversationBlockRow` 仍作为组件名存在。

判断：

这些不是旧 raw event block reducer，但会继续保留 block 语义。严格按计划“替换顶层 block model”和“不要留下 wrapper compatibility shims”读，仍未完成。

### Task 7: Replace Event Reducer With Projection Store

状态：部分完成。

已完成：

- store 状态持有 `turns`、`pageCursor`、`eventCursor`。
- `hydrateWorktree` 只接受 worktree page。
- optimistic turn 用 `clientMessageId` 与 projected turn 对齐。
- event stream source 只把 raw event batch 转成 `worktreeRefreshRequested` / `markGap`，不把 raw event 组装成 UI 结构。
- terminal event 会触发 immediate refetch。

缺口：

- 没看到计划要求的 “100 streaming raw-event batches 不产生 100 次 worktree IPC” 直接测试。
- 当前有 `coalesceTimelineActions` 和 500ms throttle 实现，但缺 IPC 调用次数压力断言。

### Task 8: Build Turn Work Tree Components

状态：已完成。

证据：

- `ConversationTurnView`、`AssistantWorkView`、`ThinkingPanel`、`AssistantTextSegmentView`、`ToolGroupSegmentView`、`ToolAttemptRow`、`PermissionInlinePanel`、`ArtifactSegmentView`、`ReviewRequestSegmentView`、`ClarificationRequestSegmentView` 都存在。
- `AssistantWorkView` 按 segment kind 渲染。
- tool status 和 permission status 分开显示。
- `Details` callback 使用 `eventRef`。
- UI 测试覆盖一个 assistant work tree、嵌套 tools、permission、safe failure summary、review、clarification。

本次验证：

```bash
pnpm -C apps/desktop test src/features/conversation/ConversationWorkspace.test.tsx src/features/conversation/timeline/conversation-timeline.test.tsx
# 2 files passed, 13 tests passed
```

### Task 9: Localize Conversation Work Tree UI

状态：部分完成。

已完成：

- `en-US.ts` 和 `zh-CN.ts` 有 conversation timeline 文案。
- 中文测试覆盖 `执行：失败`、`权限：已批准`，并确认不显示 `Execution: failed`、`Permission: approved`、`completed`、`approved`。

缺口：

- 计划要求的精确 key，如 `assistant.status.running`、`tools.title`、`details.viewRawEvents` 不存在。
- 实现使用的是 `timeline.toolStatus.*`、`timeline.tools`、`timeline.details`。
- 中文测试没有完整覆盖计划列出的禁用英文：`Tools`、`Approved`、`Complete`、`failed`。

### Task 10: Update Storybook State Matrix

状态：部分完成。

已完成：

- `conversation-timeline.stories.tsx` 使用 `ConversationTurn` fixture。
- 已有 stories：
  - `Empty`
  - `Streaming`
  - `PermissionPending`
  - `ToolFailed`
  - `ArtifactReady`
  - `ReviewAndClarification`
  - `RunFailed`
  - `LongConversation`
- `docs/frontend/frontend-quality.md` 提到 worktree component stories。

缺口：

- 缺计划要求的独立状态：
  - Simple completed turn
  - Tool approved and completed
  - Multiple attempts for one tool
  - Tool-call-only assistant message does not show empty text
  - Withheld thinking 独立 story
  - Final answer after failed tool
- 未运行 `pnpm -C apps/desktop build-storybook`，未见通过证据。

### Task 11: Remove Obsolete Event-Block Code

状态：部分完成。

已完成：

- 以下旧文件已不存在：
  - `conversation-timeline-reducer.ts`
  - `conversation-timeline-index.ts`
  - `conversation-timeline-thinking.ts`
  - `conversation-block-renderer.tsx`
- timeline 目录没有 `RunEvent` 命中。
- 旧顶层 block 类型名基本清理。

缺口：

- `ConversationBlockRow` 仍存在。
- `blocks` / `pendingPermissionBlocks` 仍作为 API alias 存在。
- 没运行 `pnpm -C apps/desktop knip`。

### Task 12: Update Product And Engineering Docs

状态：部分完成。

已完成：

- frontend product doc 已写明 canvas 使用 `ConversationTurn[]`，raw `RunEvent` 不进入主 canvas。
- backend runtime doc 已写明 worktree projection 由 Rust 所有。
- backend engineering doc 已列出 `page_conversation_worktree`。
- backend quality doc 已列出 conversation worktree 覆盖要求。

缺口：

- `docs/frontend/frontend-engineering.md` 前半段说 `page_conversation_worktree` 是 `ConversationCanvas` 数据源。
- 同一文件后半段仍写：
  - `ConversationBlock[]` is the only render source for the conversation canvas.
  - `get_conversation`, replay events, live events, artifacts, local submits, command results feed the timeline reducer.
  - streaming assistant text stays in reducer buffer until `assistant.completed`。
- 这是明确的文档自相矛盾。

### Task 13: End-To-End Regression Tests

状态：部分完成。

已完成：

- `ConversationWorkspace.test.tsx` 覆盖 worktree 渲染、withheld placeholder 不进 UI、嵌套 permission 决策、terminal event refetch。
- `conversation-timeline.test.tsx` 覆盖一个 assistant work tree、nested tools、details callback、review/clarification。
- Tauri command test 覆盖安全 tree。

缺口：

- 计划中的完整 MiniMax 截图事件流没有完整 UI fixture。
- backend command test 覆盖了部分事件流，但没有完整包含多轮 tool completed/failed、assistant delta、retry text、run.ended 的端到端场景。

### Task 14: Full Gates

状态：未完成。

本次已跑：

```bash
cargo test -p jyowo-harness-contracts conversation_worktree --test m1_contracts
cargo test -p jyowo-harness-contracts schema_export --test m1_contracts
cargo test -p jyowo-harness-journal --test conversation_worktree_projector
cargo test -p jyowo-harness-journal --features sqlite --test conversation_read_model sqlite_conversation_read_model_projects_worktree
cargo test -p jyowo-harness-sdk --features testing conversation_read_model_facade_returns_worktree_page --test conversation_read_model
cargo test -p jyowo-desktop-shell --test commands page_conversation_worktree
pnpm -C apps/desktop test src/features/conversation/conversation-production-boundaries.test.ts
pnpm -C apps/desktop test src/shared/tauri/commands.test.ts src/features/conversation/timeline/conversation-timeline-selectors.test.ts src/features/conversation/timeline/conversation-timeline-store.test.ts src/features/conversation/timeline/conversation-timeline-source.test.ts src/features/conversation/timeline/conversation-timeline.test.tsx
pnpm -C apps/desktop test src/features/conversation/ConversationWorkspace.test.tsx src/features/conversation/timeline/conversation-timeline.test.tsx
```

本次 focused tests 通过。

本次未跑：

```bash
pnpm check:docs
pnpm check:desktop
pnpm check:rust
pnpm check
pnpm -C apps/desktop build-storybook
pnpm -C apps/desktop test:e2e
pnpm -C apps/desktop knip
```

因此不能判定 Task 14 完成。

## 漂移检查结果

### RunEvent

```bash
rg -n "RunEvent" apps/desktop/src/features/conversation/timeline
```

结果：无命中。

判断：timeline 产品模型没有直接导入 `RunEvent`。

### 计划中的第一条 drift rg

```bash
rg -n "permissionRequest|PermissionRequestBlock|Tool error withheld from conversation timeline" apps/desktop/src/features/conversation apps/desktop/src/shared
```

结果：有命中。

命中包括：

- `apps/desktop/src/shared/events/run-event-schema.ts` 中合法 raw event schema。
- `apps/desktop/src/features/conversation/conversation-production-boundaries.test.ts` 中防回归断言。
- `apps/desktop/src/shared/tauri/commands.test.ts` 中测试变量名。
- `apps/desktop/src/features/conversation/ConversationWorkspace.test.tsx` 中 UI 不显示 withheld placeholder 的断言。

判断：该 drift guard 过宽，不能直接作为 pass/fail 信号。需要改成只扫描 canvas 生产渲染/model 文件，或显式排除 raw event schema 和 tests。

### 计划中的旧 block drift rg

```bash
rg -n "AssistantMessageBlock|ToolGroupBlock|ThinkingBlock|assistantStreaming|toolGroup" apps/desktop/src/features/conversation/timeline
```

结果：有命中。

命中包括合法 segment kind `toolGroup`。

判断：`toolGroup` 现在是合法 `AssistantSegment` kind。该 drift guard 需要区分旧顶层 `ToolGroupBlock` 和新嵌套 `ToolGroupSegment`。

### get_conversation.messages

```bash
rg -n "get_conversation.messages|messages:" apps/desktop/src/features/conversation apps/desktop/src/shared/tauri
```

结果：有命中。

命中集中在：

- `shared/tauri` 的 `get_conversation` schema、mock、tests。
- `use-conversation-timeline.ts` 的 draft conversation metadata。
- `ConversationWorkspace.test.tsx` 的 mock conversation。

判断：未看到 `get_conversation.messages` 进入 canvas timeline。计划中的 rg 同样过宽，不能直接作为 pass/fail。

## 当前最高优先级缺口

1. 补 Rust projector 对 artifact/review/clarification/notice 的投影和测试。
2. 修正文档残留：`docs/frontend/frontend-engineering.md` 的 `ConversationBlock[]` / reducer 旧描述。
3. 移除或重命名前端兼容 block API：`blocks`、`pendingPermissionBlocks`、`ConversationBlockRow`。
4. 补 Task 7 的 100 batch 节流 IPC 测试。
5. 补 Storybook 状态矩阵并跑 `build-storybook`。
6. 增加 `page_conversation_worktree` malformed id fail-closed test 和 schema parity test。
7. 运行 full gates：`pnpm check:docs`、`pnpm check:desktop`、`pnpm check:rust`、`pnpm check`。

## 当前可认为已满足的核心不变量

- 主 canvas 当前读取 `ConversationTurn[]`。
- `ConversationCanvas` 没看到使用 `get_conversation.messages` 作为数据源。
- timeline 目录没有 `RunEvent`。
- tool permission 嵌套在 tool attempt 下。
- tool failure summary 在 projector 和 Tauri test 中是安全文案。
- raw withheld placeholder 没有进入主要 UI 测试路径。
- empty assistant text segment 有后端和前端测试覆盖。

## 当前不能认为已满足的不变量

- Rust projector 处理全部计划 segment 类型。
- snapshot/live/gap 全部共享同一投影语义并有完整测试。
- worktree materialized tables 存在。
- Storybook 覆盖全部要求状态。
- full gates 全部通过。
- drift guard 可以直接作为自动门禁。
