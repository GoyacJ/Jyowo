# Jyowo AI 执行规则

本文件适用于所有能读取仓库说明的 AI agent。

目标是让 agent 在 Jyowo 仓库内执行任务时，先读规范，再改文件，再验证结果。

## 读取顺序

开始任何任务前，先读本文件。

涉及前端时，按顺序读：

```text
docs/frontend/agent-harness-frontend-development-guidelines.md
docs/frontend/frontend-product-ux.md
docs/frontend/frontend-engineering.md
docs/frontend/frontend-quality.md
```

涉及后端时，按顺序读：

```text
docs/backend/agent-harness-backend-development-guidelines.md
docs/backend/backend-runtime.md
docs/backend/backend-engineering.md
docs/backend/backend-quality.md
```

跨前后端任务必须同时读取两组规范。

仓库内如果出现更深层的 `AGENTS.md`，更深层文件只补充该目录规则，不取消根规则。

## 执行规则

默认按这个顺序执行：

```text
理解需求
读取相关规范和代码
列出可验证目标
做最小必要修改
运行对应门禁
复核 diff
说明结果
```

规则：

- 不先写实现再补理由。
- 不跳过规范文件。
- 不把文档写成计划表。
- 不把临时状态写进规范文档。
- 不做无关重构。
- 不回滚用户或其他 agent 的改动。
- 不提交生成物噪音。
- 不引入未使用的依赖、import、变量或文件。

## 修改边界

改动必须靠近任务本身。

前端代码放在 `apps/desktop/src` 的既有层级内。

Rust 后端代码放在现有 workspace crate 内。

公共 contract 优先放在 `crates/jyowo-harness-contracts`。

Tauri command 只作为 IPC 边界。业务逻辑留在 harness runtime 或 SDK facade。

新增规范必须接入 docs gate。不能只新增 Markdown 文件。

## 前端规则

前端规范以这些文件为准：

```text
docs/frontend/agent-harness-frontend-development-guidelines.md
docs/frontend/frontend-product-ux.md
docs/frontend/frontend-engineering.md
docs/frontend/frontend-quality.md
```

执行前端任务时必须保持：

- React 只展示状态和发起请求。
- 最终安全决策留在 Rust。
- Tauri IPC 只能通过 `shared/tauri` 和 `CommandClient` 暴露。
- 外部 payload 必须用 Zod 校验。
- `shared` 不依赖 `app`、`routes`、`features`。
- `features` 不依赖 `app`、`routes`。
- `@chenglou/pretext` 只能通过 `shared/text-layout` 使用。
- 复杂业务 UI 必须覆盖 loading、empty、error、ready 状态。

## 后端规则

后端规范以这些文件为准：

```text
docs/backend/agent-harness-backend-development-guidelines.md
docs/backend/backend-runtime.md
docs/backend/backend-engineering.md
docs/backend/backend-quality.md
```

执行后端任务时必须保持：

- Rust backend 是 Policy authority。
- `harness-contracts` 是 public serde contract 源头。
- workspace 依赖方向保持 `Tauri shell -> L4 -> L3 -> L2 -> L1 -> L0`。
- `PermissionBroker` 不能被 Tool、filesystem、network、sandbox、MCP 或破坏性操作绕过。
- `Redactor` 必须在 Journal、Replay、logs、traces、export 前执行。
- public payload 必须有稳定 `serde` shape。
- 稳定 schema 使用 `JsonSchema`。
- `unsafe_code = "forbid"` 必须保留。

## 安全边界

默认 fail-closed。

适用范围：

- permission 缺失
- Secret 暴露风险
- sandbox 能力缺失
- tenant 或 workspace scope 异常
- Tauri command payload 异常
- MCP tool origin 不明确
- Journal 或 Replay 可见性异常

Secret 不得进入：

```text
prompt
event
log
trace
test snapshot
screenshot
frontend state
```

允许 fail-open 的只能是非安全遥测。代码旁必须解释原因，并有测试。

## 质量门禁

根级门禁：

```text
pnpm check
pnpm check:docs
pnpm check:agent-docs
pnpm check:frontend-docs
pnpm check:backend-docs
pnpm check:desktop
pnpm check:rust
```

Rust 门禁：

```text
cargo fmt --all --check
cargo check --workspace
cargo test --workspace
```

修改前端代码后至少运行：

```text
pnpm check:desktop
```

修改 Rust 后端代码后至少运行：

```text
pnpm check:rust
```

修改规范、脚本或 AGENTS 文件后至少运行：

```text
pnpm check:docs
```

跨前后端改动运行：

```text
pnpm check
```

## 提交前自检

提交或交付前检查：

- 是否读过相关规范。
- 是否只改了任务需要的文件。
- 是否没有留下孤儿代码。
- 是否没有绕过安全边界。
- 是否更新了相关 docs。
- 是否新增或更新了必要测试。
- 是否运行了对应门禁。
- 是否确认命令退出码为 0。
- 是否说明了未完成或未验证的部分。
