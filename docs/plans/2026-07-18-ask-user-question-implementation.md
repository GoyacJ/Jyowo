# Ask User Question 工具实施计划

## 目标

完成工具契约、daemon 状态、IPC 命令、前端表单、恢复语义和竞争测试，使模型可以在当前任务运行中等待并消费用户的结构化回答。

## 实施步骤

### 1. Contracts 与协议

- 增加 Ask User Question 请求、问题、选项、回答和结果类型。
- 增加 pending question projection、waiting input 状态和 resolve question 命令。
- 更新 daemon schema export 并重新生成前端协议类型。

### 2. Task event 与 projection

- 增加 requested、resolved、invalidated task event。
- 持久化 pending question projection。
- 实现 Running、WaitingInput、Yielding 和 restart 的合法状态流转。
- 将 waiting input 纳入 active run、队列、stop、promotion 和恢复判断。

### 3. QuestionBroker

- 实现 request、resolve、timeout 和 invalidate。
- 维护 request-scoped validation context 与 waiter。
- 处理重复回答、错误 revision、超时竞争和提交后唤醒。
- 在父前台 SDK runtime 中注入 run-bound capability。

### 4. 工具

- 将 `Clarify` 重命名为 `AskUserQuestion`。
- 使用多问题输入和结构化状态输出。
- 将批量上限收紧为 3，并在工具描述中要求默认单题、只询问阻塞项；仅允许批量提交相互独立的短问题。
- 改为非升级授权并移除工具自行生成但无法关联的 clarification journal event。
- 仅在 capability 和 fully-interactive 条件满足时进入工具池。

### 5. 前端

- 在任务工作区增加 pending question 表单。
- 通过 resolve question daemon command 提交回答或拒绝。
- waiting input 时普通 Composer 保持 queue 模式。
- 限制表单高度，问题区独立滚动，固定标题与操作区。
- 使用 radio、checkbox 和按需展开的“其他”回答，明确必填、回答类型和完成进度。
- 增加中英文文案、加载、错误和过期状态。

### 6. 恢复与验证

- stop、promotion、timeout 和 daemon recovery 失效 pending question。
- 覆盖单选、多选、自由文本、拒绝和超时。
- 覆盖双客户端竞争、错误 request/revision 和重连恢复。
- 运行生成器、窄测试、`pnpm check:quick`，最后按环境能力运行完整检查。
