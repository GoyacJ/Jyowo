# Composer Context Bar 完整实施计划

## Summary

实现发送框左侧按钮为统一的 Composer Context Bar：

- 纸夹：添加附件。
- `@`：引用项目对象和运行时能力。
- 前端只展示 draft 和 chip。
- Rust 校验、存储、解析、注入上下文。
- 不实现命令模式。

## Public Interfaces

在 `crates/jyowo-harness-contracts` 提供稳定 contract：

- `ConversationContextReference`
- `ConversationAttachmentReference`
- `ConversationTurnInput`

`ConversationContextReference` 支持：

- `workspace_file`
- `artifact`
- `conversation`
- `memory`
- `skill`
- `tool`
- `mcp_server`

更新 Tauri `start_run` payload：

- `conversationId`
- `prompt`
- `contextReferences?: ContextReference[]`
- `attachments?: AttachmentReference[]`

新增 Tauri commands：

- `createAttachmentFromPath({ path })`
- `listReferenceCandidates()`

## Runtime Flow

`ConversationTurnRequest` 携带 `ConversationTurnInput`。

`start_run_with_runtime_state` 负责：

1. 校验 `conversationId` 和 prompt。
2. 校验 context references。
3. 校验 attachment id、大小和 workspace scope。
4. 构造 runtime turn input。
5. 交给 `submit_conversation_turn`。
6. SDK 在 Rust 内部把引用和附件解析成受控 context block，再进入模型输入。

前端不拼 prompt。

## UX Defaults

- chips 显示在 textarea 下方、toolbar 上方。
- chip 可删除。
- 重复引用去重。
- `@` picker 支持搜索。
- 引用分组显示 Files / Artifacts / Conversations / Memories / Skills / Tools / MCP Servers。
- 文件选择错误显示在 composer 内。
- 超大文件错误文案明确显示限制。
- icon-only buttons 必须有 `aria-label` 和 tooltip。

## Security Defaults

- Rust 是唯一安全边界。
- 前端不读取文件内容。
- workspace 文件必须 canonicalize 并确认在 workspace 内。
- workspace 外文件必须复制进受控 attachment store。
- 默认单文件 5MB，总附件 20MB。
- 非文本文件第一版只保存 metadata。
- skill、tool、MCP server 引用必须在 Rust 侧确认存在。
