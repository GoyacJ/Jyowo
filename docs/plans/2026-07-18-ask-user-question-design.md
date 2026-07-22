# Ask User Question 工具设计

## 目标

将现有仅有工具定义的 `Clarify` 改造成可在桌面任务中完整工作的 `AskUserQuestion`：模型发起一个或多个问题，daemon 持久化待回答状态，前端提交结构化回答，工具收到结果后继续当前运行。

## 核心决策

- daemon 是待回答状态、竞争处理、超时、失效和恢复语义的唯一所有者。
- 工具正式名称为 `AskUserQuestion`，不再向模型暴露 `Clarify`。
- 默认一次只问一个阻塞继续执行的问题；只有问题彼此独立、简短且适合一次回答时，才允许批量提问。
- 一次调用包含 1–3 个问题；每个问题支持单选、多选和自由文本。能从上下文推断或采用安全默认值的信息不再询问。
- 每个问题只表达一个决策维度；单选项必须互斥，可能同时成立的选项必须使用多选或拆成不同问题。
- 工具调用本身不构成授权，使用非升级授权；后续危险动作仍单独经过权限系统。
- 只有 fully-interactive 的前台运行暴露该工具；无交互运行不注册对应 capability。
- 一个任务同一时间只允许一个待回答请求。
- 问题请求和回答都通过 task event 持久化；内存 waiter 只负责唤醒当前工具 future。
- 普通 Composer 在等待回答期间继续进入消息队列，不能代替结构化回答命令。
- 默认等待时间由宿主配置控制，模型不能修改。

## 工具契约

输入包含 `questions` 数组。每个问题包含稳定 ID、标题、问题正文、选项、是否多选和是否允许自定义回答。问题 ID 与选项 ID 在单次调用内必须唯一。

输出包含 `status` 和逐问题回答。`status` 为 `answered`、`declined`、`timed_out` 或 `cancelled`。用户拒绝或超时属于正常工具结果，不作为基础设施错误。

## 状态模型

新增 `TaskState::WaitingInput`、`RunState::WaitingInput` 和 `TaskProjection::pending_question`。

持久事件：

- `question.requested`
- `question.resolved`
- `question.invalidated`

状态流转：

```text
Running -> WaitingInput -> Running
WaitingInput -> Yielding       (stop / promotion)
WaitingInput -> Interrupted    (daemon restart)
```

回答命令携带 task ID、request ID、request revision、回答内容和现有 command metadata。首个成功提交的回答生效；其他客户端收到 stale/invalid transition。

## Daemon broker

`QuestionBroker` 采用与 permission broker 相同的提交后唤醒模式：

1. 注册 waiter 和原始校验上下文。
2. 事务提交 `question.requested`。
3. 等待回答或宿主超时。
4. 回答命令校验 request identity、revision 和答案结构。
5. 事务提交 `question.resolved`。
6. 提交成功后唤醒 waiter。

stop、promotion、timeout 和 recovery 必须先提交 invalidation，再唤醒 waiter。超时与回答竞争时，以已提交的持久结果为准。

## 重启语义

- 重启时仍待回答的请求失效，旧页面不能继续提交。
- 当前 run 按既有规则进入 `InterruptedByRestart`。
- 如果回答已经提交但 tool completion 尚未写入，恢复应重用已提交答案，不能再次询问用户。

第一阶段采用有限等待时间。当前运行在等待期间仍占用 foreground permit；跨小时或跨天等待需要独立的挂起和恢复机制，不属于本次范围。

## 前端

在 pending permission 相同区域渲染 `PendingQuestionForm`。断线重连后完全从 `TaskProjection.pending_question` 恢复。

- 表单高度受工作区约束，问题列表独立滚动；标题和操作区始终可见。
- 每个问题明确显示必填以及单选、多选或文本回答类型。
- 单选使用 radio，多选使用 checkbox，不依赖颜色表达选中状态。
- 有预设选项时，自定义回答以“其他”选项按需展开，避免每题常驻大文本框。
- 批量问题显示完成进度；提交不可用时说明剩余未回答数量。
- 用户可以提交完整回答，或暂不回答整个请求。错误状态留在固定操作区中。

## MCP elicitation

现有 MCP stream elicitation 暂不重构。Ask User Question 稳定后可将 MCP form elicitation 接入同一 daemon interaction 状态，避免本次实现被通用表单抽象阻塞。

## 非目标

- 不把问题回答当作工具权限授权。
- 不支持无限期等待。
- 不在首个版本中允许后台 agent 或自动化直接阻塞等待用户。
- 不改变普通消息队列的调度语义。
