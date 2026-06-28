# Codex Style Conversation Evidence UI Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 将 Jyowo 对话页重构为 Codex 风格的证据链式工作流界面，让用户在主对话里看见 agent 的意图、进度、文件修改、命令输出、失败原因和最终结论。

**Architecture:** `RunEvent` 仍是 durable fact log。Rust 继续负责 UI-safe `ConversationTurn[]` projection。React 只负责把 `ConversationTurn -> AssistantWork -> ProcessStep` 渲染成自然对话、状态行和证据块，不从 Raw Event 拼产品结构。

**Tech Stack:** React 19, TypeScript 6, Tailwind CSS v4, TanStack Virtual, Storybook 10, Vitest, Testing Library, Playwright, Rust, serde, schemars, Zod, shiki, lucide-react.

## Current Status And Supplement

首轮实施后发现仍有若干验收缺口：上下文连续性、provider 身份约束、zh-CN 固定 runtime 文案、历史步骤折叠聚合、完成 run 内失败步骤表达、composer 底部遮挡风险，以及普通正文被路径脱敏误伤的风险。

这些缺口不推翻本计划。补充计划记录在 `docs/plans/2026-06-28-codex-style-conversation-evidence-ui-supplement.md`。后续实现和验收必须同时对照本计划和该 supplement。

---

## Design Intent

这次重构贴合 Codex 的设计方式，不复制 Codex 品牌。

贴合的是这套产品结构：

```text
用户给目标
assistant 在同一条对话里展示执行过程
执行过程由状态行和证据块组成
证据块承载代码 diff、shell 输出、工具结果、附件和失败状态
细节可以折叠，但关键失败和当前动作默认展开
最终回答收束主线
```

不贴合这些内容：

```text
Codex logo
Codex 精确文案
OpenAI 品牌色
把主画布改成 Raw Event 流
把 Activity / Replay / Raw JSON 提到主视觉层
```

Jyowo 继续保留现有产品约束：

- 主模型是 `ConversationTurn[]`。
- React 只展示状态和发起用户意图。
- 安全决策、脱敏、payload 校验留在 Rust。
- 复杂业务 UI 覆盖 loading、empty、error、ready、running、failed。
- 视觉使用 semantic tokens，不在 feature 代码硬编码品牌色。

## Reference Anatomy

Codex 截图里的界面由这些层组成：

```text
Turn
├─ User Bubble
│  ├─ Attachment thumbnails
│  ├─ User text
│  ├─ Timestamp
│  └─ Copy affordance
└─ Assistant Flow
   ├─ Assistant status duration row
   ├─ Natural-language progress text
   ├─ File edit status row
   ├─ Diff evidence block
   ├─ Command status row
   ├─ Shell evidence block
   ├─ Collapsed historical commands
   ├─ Context compaction notice
   └─ Final narrative answer
```

页面信息层级：

1. 用户目标和 assistant 结论。
2. 当前工作进度和失败状态。
3. 当前打开的证据块。
4. 已完成的历史工具调用。
5. Raw details、Replay、JSON。

主画布只显示前四层。第五层进入 Activity / Details。

## Screenshot-Level Product Audit

后续实现必须按截图里的产品语法落地。不能只做“黑色卡片 + 代码块”。

### Shared Frame

三张截图共同体现的是一个窄阅读列，而不是全宽 dashboard。

必须保留：

- 主内容列居中。
- 对话内容不铺满窗口。
- 每个 turn 之间有明确纵向呼吸。
- assistant 侧是文档流。
- user 侧是右对齐气泡。
- 证据块只包住证据本身。
- 页面不能出现灰色卡片堆。
- 执行状态行比正文更弱。
- 当前证据块比历史状态行更强。
- 文本、状态、证据块共用同一条时间线。

禁止：

- 把每个状态都做成独立大卡片。
- 把 diff / shell 放进二级嵌套卡片。
- 把 Activity rail、Raw JSON、Replay 放到主画布。
- 用装饰性大标题、hero、营销式说明。

### Screenshot 1: Edited File, Failed Test, Context Compaction

可见结构：

```text
status group: 已编辑 1 个文件
  group title: 已编辑的文件
  diff block: SkillsPage.test.tsx +61 -2
assistant text: RED 测试已就位...
collapsed status: 已运行 3 条命令
expanded status: 已运行命令，已持续 12s
shell block
collapsed historical command rows
context compaction divider
assistant text: 先接着现有红测状态...
```

设计点：

- 文件编辑状态行使用小图标、低对比文字、chevron。
- diff block 顶部有独立 header。
- diff block header 左侧是文件名，右侧是复制图标。
- diff block header 的高度紧凑，约 28-32px。
- diff block 内部有独立滚动条。
- diff 左边有强色竖条，标记变更区域。
- 行号 gutter 独立于代码内容。
- added 区域使用绿色底色，不只改文字颜色。
- 代码内容使用语法高亮。
- shell block 的外形和 diff block 不同。
- shell block header 显示 `Shell`。
- shell output 用 terminal 背景。
- shell 失败不使用整块红底，只在 exit code 和失败行体现。
- shell block 右下角显示 `退出码 1`。
- 历史命令是低对比单行，不抢当前 shell 的视觉层级。
- context compaction 是横线分隔，不是 warning card。

实现约束：

- `fileEdit` 状态行不能和 `diff` block 合并成同一个标题。
- failed command 必须默认展开。
- completed historical command 默认可以折叠。
- context compaction 必须用结构化 notice code，不能靠正文匹配。

### Screenshot 2: Narrative, Read File, Inline Code, Compact Diff

可见结构：

```text
assistant paragraph
status row: 已编辑 1 个文件已搜索代码
assistant paragraph
status row: 已读取 1 个文件
read file label: Read skill_catalog.rs
assistant paragraph with inline code
status row: 已编辑 1 个文件
diff status: 已编辑 skill_catalog.rs +28 -10
diff group title: 已编辑的文件
diff block
assistant paragraph
```

设计点：

- assistant 正文是主线，不放在气泡里。
- 状态行夹在正文之间，像执行脚注。
- 文件名链接使用蓝色，和普通文字区分。
- inline code 是灰色胶囊，不是完整代码块。
- 读文件状态只显示必要文件名，不展开文件内容。
- diff block 比正文更窄的视觉重量，但和正文左边缘对齐。
- diff 内 removed 行是红底，added 行是绿底。
- 同一行号区域可同时显示旧行和新行。
- diff header 文件名显示短名，不显示冗长绝对路径。

实现约束：

- 读取文件步骤不应强制渲染为大证据块。
- `ProcessStepKind.FileRead` 默认是 status row + compact label。
- inline code 样式必须由 Markdown renderer 统一管理。
- 文件路径必须显示 project-relative 或 safe label，不显示私有绝对路径。

### Screenshot 3: User Attachments, Bubble, Assistant Analysis

可见结构：

```text
attachment strip
user bubble
timestamp + copy affordance
assistant duration row: 已处理 12m 46s
divider
assistant markdown answer
inline code capsules
file links
```

设计点：

- 附件缩略图在 user bubble 上方。
- 缩略图横向排列。
- 缩略图有圆角、边框和阴影。
- user bubble 右对齐。
- user bubble 宽度约为主列 70%。
- timestamp 和 copy 图标在 bubble 外侧。
- assistant duration row 是低对比元信息。
- duration row 后有分隔线。
- assistant 回答不包卡片。
- markdown 列表层级清楚。
- inline code 胶囊比正文更亮。
- 文件链接使用图标 + 蓝色链接。

实现约束：

- 历史附件必须来自 `ConversationTurnUserMessage.attachments`。
- 不得从 composer draft 推断历史附件。
- 如果没有安全缩略图，必须退化为 file chip。
- assistant duration 没有 contract 字段时，不显示假时长。
- 文件链接必须来自安全引用字段，不能从正文正则解析私有路径。

## Product Interaction Contract

### Disclosure Rules

默认展开：

- 当前 running step。
- failed step。
- permission pending step。
- 最近一个 shell command。
- 最近一个 diff block。

默认折叠：

- 成功完成且无输出的 tool attempt。
- 非当前的 completed command。
- 多个 file read/search 聚合项。
- 超过 2 个的 completed low-signal steps。

禁止折叠：

- non-zero exit code 的 command block。
- permission denied / failed。
- failed tool attempt。
- redaction / withheld notice。

### Collapse State Ownership

折叠状态是 UI-only state。

存放位置：

```text
shared/state/ui-store.ts
```

key 规则：

```text
conversationId + assistant.runId + segment.id + step.id
```

行为：

- 用户手动展开后，在当前 conversation 生命周期内保持展开。
- conversation 切换后可以保留。
- app 重启后不要求恢复。
- running -> failed 时强制展开。
- running -> complete 时按 disclosure rules 重新计算默认状态，除非用户手动改过。

### Copy Affordance

截图中的复制按钮是证据块和 user bubble 的低噪声操作。

规则：

- 默认可见但低对比，hover/focus 时增强。
- icon 使用 lucide `Copy`。
- 必须有 `aria-label`。
- 必须有 tooltip。
- copy 成功可以用 toast，不在证据块内插入成功文案。

Copy payload:

- diff: 复制当前 diff preview。
- shell: 复制 command + output + exit code + duration。
- user bubble: 复制用户正文，不复制附件内容。

### Scrollbar And Overflow

截图中的 diff/shell 都是块内滚动。

规则：

- 页面主滚动只负责 conversation timeline。
- diff/shell 长内容使用块内滚动。
- 块内滚动条不能遮住 copy button。
- 横向滚动只在代码 viewport 内发生。
- evidence block 不得撑宽主列。
- mobile 下代码块仍保持横向滚动，不强制换行破坏代码结构。

## Non-Negotiable Invariants

- `ConversationTimeline` 输入必须是 `ConversationTurn[]`。
- `features/conversation/timeline` 不得导入 `RunEvent`。
- `get_conversation.messages` 不得驱动主画布。
- `ToolPermissionState` 必须嵌在所属 `ToolAttempt` 下。
- `ProcessStep.detail` 是证据块的唯一数据来源。
- React 不解析 shell 原始命令权限 payload。
- React 不展示 raw chain-of-thought。
- 私有路径、secret、未脱敏 payload 不进入主画布。
- diff、shell、artifact、permission 的失败状态必须可见。
- Activity rail 是辅助层，不替代 Conversation Canvas。
- Storybook 必须包含 Codex-style 复合场景。
- Playwright 必须截图验证 system、light、dark 三种模式下证据块不重叠、不空白。
- Color mode follows Jyowo theme settings and OS preference; Codex dark screenshots are not a dark-only mandate.
- Every image-derived design point in this plan must have either a component test, Storybook story, or screenshot target.
- User bubble must stay neutral and quiet. Primary brand fill is reserved for links, actions, and active affordances.
- Adjacent low-signal process steps may aggregate into one status row when that preserves the screenshot rhythm.
- New UI-visible data fields must originate in Rust contracts, journal/read-model projection, and frontend Zod schemas together. React props must not invent unsupported contract fields.

## Target Visual Contract

### Layout

Conversation page:

```text
AppShell
├─ Sidebar
├─ Main
│  ├─ Top actions
│  ├─ ConversationTimeline
│  │  ├─ Header
│  │  ├─ Scroll viewport
│  │  └─ Jump to latest
│  └─ Composer
├─ ContextPanel
└─ ActivityRail
```

Main canvas:

- Max width: `900px` to match current Jyowo canvas and Codex reading width.
- Turn gap: `20px`.
- Assistant flow max width: `86%` on desktop, `100%` below `720px`.
- User bubble max width: `78%` on desktop, `92%` below `720px`.
- Evidence block width: fills assistant flow width.
- Evidence block radius: `8px`.
- Evidence block border: `border`.
- Nested card inside card is forbidden.

### Theme

Default app theme remains governed by current Jyowo theme settings.

Codex screenshots are dark, but Jyowo must follow system/community theme behavior.

Required modes:

```text
system
light
dark
```

Theme rules:

- System mode follows OS preference.
- Light and dark are manually selectable.
- Codex is used as layout and hierarchy reference, not as dark-only requirement.
- Evidence components must be token-driven.
- Dark mode should match the screenshot posture: low-glare background, raised evidence surfaces, green/red semantic diff.
- Light mode should keep the same hierarchy: quiet status rows, stronger evidence blocks, no dashboard cards.
- No feature component may branch on raw hex colors.
- No feature component may assume dark mode for contrast.

Required semantic roles:

```text
surface                 normal conversation surface
muted                   quiet rows and inactive status
code-background         diff/code surface
terminal-background     shell surface
success                 added lines and success status
destructive             removed lines and failed status
warning                 running/attention status
primary                 active links and details affordance
border                  evidence block borders
muted-foreground        metadata text
```

Feature components must use Tailwind token classes such as `bg-surface`, `bg-code-background`, `bg-terminal-background`, `text-success`, `text-destructive`, `border-border`.

Visual mode acceptance:

```text
system: renders with the active OS preference
light: all evidence blocks remain readable, diff red/green is visible but restrained
dark: matches screenshot hierarchy without copying exact Codex color values
```

### Typography

- Conversation body: `text-sm`, `leading-6`.
- Assistant progress text: `text-sm`, `leading-6`, `text-foreground`.
- Status rows: `text-xs`, `leading-5`, `text-muted-foreground`.
- Evidence header: `text-xs`, `font-medium` or `font-mono` where filename/command matters.
- Shell output: `font-mono`, `text-xs`, `leading-5`.
- Diff output: `font-mono`, `text-[12px]`, `leading-5`.
- Page title stays compact; no hero-sized text.

### Measurement Guardrails

These are implementation guardrails, not exact pixel cloning.

```text
conversation max width      900px
turn vertical gap           20px
assistant segment gap       12px
status row height           24-32px
evidence header height      28-34px
evidence radius             8px
evidence border             1px
diff max height             360px
shell max height            260px
thumbnail size              72px x 72px desktop, 56px x 56px compact
copy icon button            28px x 28px
inline code radius          4px
```

Spacing rules:

- Status row sits 8-10px away from adjacent prose.
- Evidence block sits 6-8px below its owning status row.
- Final assistant prose after evidence has at least 12px top spacing.
- User attachment strip has 8px gap between thumbnails.
- Timestamp/copy row has 6px top spacing below user bubble.

### User Bubble

User message must render as a quiet right-aligned bubble.

Required details:

- Bubble background: quiet neutral surface, using `muted` or a surface token variant in light and dark theme.
- Do not use `primary` as the bubble fill. The screenshots show a neutral message surface, not a brand-colored chat bubble.
- Text: `foreground`.
- Radius: `8px`.
- Padding: `12px 16px`.
- Attachments, when present, render above text as thumbnails or file chips.
- Timestamp/copy affordance sits outside or below the bubble with low contrast.
- Long words and file names must wrap or truncate without expanding the layout.
- Attachment strip aligns to the bubble's right edge, not the full conversation column.

### Assistant Flow

Assistant output must feel like a document stream, not a stack of generic cards.

Required details:

- Assistant label is small and quiet.
- Natural text uses Markdown renderer.
- Process segment begins with a summary line.
- Each process step renders either:
  - status row only,
  - status row + inline detail,
  - status row + evidence block.
- Current running step is visually visible.
- Failed step is visible without making the whole page look like an alert screen.

### Status Rows

Status rows mirror Codex rows such as “已编辑 1 个文件”, “已运行 3 条命令”, “已处理 12m 46s”.

Required row anatomy:

```text
[icon] [label] [count/status] [duration] [chevron when collapsible]
```

Rules:

- Use lucide icons.
- Icon-only buttons have accessible name and tooltip.
- Completed low-signal rows may collapse.
- Adjacent low-signal rows may aggregate, such as “已编辑 1 个文件已搜索代码”.
- Do not render every raw event as its own visible status row.
- Current row and failed row stay expanded.
- Rows use `button` only when interactive.
- Non-interactive rows use `div` or `p`, not fake buttons.

### Diff Evidence Block

Diff block must be the primary visual asset for file edits.

Required anatomy:

```text
DiffEvidenceBlock
├─ Header
│  ├─ filename
│  ├─ +added -removed
│  └─ copy button
├─ Code viewport
│  ├─ line number gutter
│  ├─ prefix gutter
│  └─ content column
└─ Hidden lines / truncation footer
```

Rules:

- Added lines use tinted success background and success text.
- Removed lines use tinted destructive background and destructive text.
- Context lines use neutral text.
- Gutter has stable width.
- Gutter uses old/new line numbers from a parsed diff view model. Do not fake line numbers from array indexes when old/new numbers are unavailable.
- Unified diff metadata lines such as `+++`, `---`, and `@@` are not added/removed content lines.
- Code content uses cached/lazy Shiki highlighting when language can be inferred from the file extension.
- If syntax highlighting is unavailable, fall back to neutral monospace text. Do not invent language colors.
- Header remains one line and truncates filename.
- Code viewport has max height.
- Horizontal overflow stays inside block.
- Copy copies visible diff preview unless full diff is available through a future contract.
- Footer text appears when `hiddenLineCount > 0`.

### Shell Evidence Block

Shell block must look like a terminal artifact, not a generic code block.

Required anatomy:

```text
CommandEvidenceBlock
├─ Header
│  ├─ "Shell" or localized label
│  └─ copy button
├─ Command line
│  └─ "$ pnpm check:desktop"
├─ Output viewport
│  └─ stdout/stderr text
└─ Footer
   ├─ duration
   └─ exit code
```

Rules:

- Shell surface uses `terminal-background`.
- Command line is visually separated from output.
- Exit code sits bottom right.
- Non-zero exit code uses destructive text.
- Long output scrolls inside the block.
- Empty output shows command and footer only.
- Output is already UI-safe from Rust; React must not run extra redaction that could hide evidence inconsistently.
- Preserve UI-safe ANSI coloring only when Rust provides safe ANSI text or structured terminal spans.
- Do not infer success/error coloring from shell text with frontend regex.
- If the contract only provides `outputSummary`, label the block as summary through visual treatment, not by adding noisy explanatory text.
- If a future contract provides full transcript, render the transcript in the same block shape.
- `stdout` and `stderr` must not be visually split unless Rust provides structured streams.

### Tool Evidence

Tool attempts render below a compact “tools” group.

Required details:

- Tool group header shows total/running/failed count.
- Each attempt row shows tool name, status, optional duration, details button.
- Permission panel is nested inside attempt row.
- Failure summary is inline under owning attempt.
- Low-signal completed tool rows can collapse under a “已运行 n 条命令 / 工具” group.

### Context Compaction Notice

Context compaction is a timeline notice, not a warning.

Required visual:

```text
────────  上下文已自动压缩  ────────
```

Rules:

- Centered text.
- Horizontal lines left and right.
- Muted color.
- Does not take a card background.

### Attachments

Historical user attachments are part of the turn.

Current gap:

- Composer has `attachments`.
- `ConversationTurnUserMessage` does not expose attachments.

Target:

```rust
pub struct ConversationTurnUserMessage {
    pub attachments: Vec<ConversationAttachmentReference>,
}
```

Frontend target:

```ts
user.attachments?: AttachmentReference[]
```

Rendering rules:

- Images render as thumbnails only when a safe preview URL/data reference exists.
- Non-image files render as compact file chips.
- Missing preview renders file chip, not broken image.
- Attachment metadata only: name, MIME type, size.
- File content never enters frontend state.
- Thumbnail images use explicit width, height, and alt text.
- More than four attachments stay in one horizontal strip with internal overflow. They must not widen the user bubble or the main column.
- Mobile uses the same horizontal overflow behavior instead of wrapping thumbnails into a tall attachment wall.

First version rule:

- If the worktree only exposes metadata, render image attachments as image-style chips, not real thumbnails.
- Real thumbnails require a separate safe preview command or existing safe artifact preview path.

## Component Map

Existing components to keep:

- `ConversationWorkspace`
- `ConversationTimeline`
- `ConversationTurnRow`
- `ConversationTurnView`
- `AssistantWorkView`
- `Composer`
- `ActivityRail`
- `ContextPanel`

Components to refactor:

- `ProcessPanel`
- `DiffViewer`
- `ToolGroupSegmentView`
- `ToolAttemptRow`

Components to add:

```text
apps/desktop/src/features/conversation/timeline/process-step-row.tsx
apps/desktop/src/features/conversation/timeline/process-status-row.tsx
apps/desktop/src/features/conversation/timeline/command-evidence-block.tsx
apps/desktop/src/features/conversation/timeline/diff-evidence-block.tsx
apps/desktop/src/features/conversation/timeline/tool-evidence-summary.tsx
apps/desktop/src/features/conversation/timeline/context-compaction-notice.tsx
apps/desktop/src/features/conversation/timeline/user-attachment-strip.tsx
apps/desktop/src/features/conversation/timeline/conversation-evidence-fixtures.ts
```

Compatibility rule:

- `DiffViewer` may remain as a wrapper if other feature code imports it.
- New process rendering should use `diff-evidence-block.tsx`.
- Do not create a generic `Card` wrapper for evidence blocks.

## Data Contract Map

Already available:

- `ConversationTurn`
- `ConversationTurnUserMessage.body`
- `AssistantWork.status`
- `AssistantSegment.Process`
- `ProcessSegment.status`
- `ProcessStep.kind`
- `ProcessStep.status`
- `ProcessStep.title`
- `ProcessStep.body`
- `ProcessStepDetail.Command`
- `ProcessStepDetail.Diff`
- `ProcessStepDetail.Tool`
- `ProcessStepDetail.Artifact`
- `ToolGroupSegment`
- `ToolAttempt`
- `ToolPermissionState`

Needed for full Codex-style parity:

- `ConversationTurnUserMessage.attachments`
- `AssistantWork.startedAt`, `AssistantWork.completedAt`, or `AssistantWork.durationMs` if assistant-level elapsed time is shown
- structured notice code such as `notice.code = 'contextCompacted'`
- parsed diff line metadata with old/new line numbers, or enough unified diff context for frontend to derive it safely
- optional `ProcessStepDetail.Command.streams` only if stdout/stderr separation is required
- optional `ProcessStepDetail.Command.ansiSpans` only if terminal coloring should be preserved without exposing unsafe escape sequences
- optional `ProcessStepDetail.Command.outputTruncated` if Rust truncates shell output
- optional `ProcessDiffFile.hiddenLineCount` if Rust provides truncation count instead of frontend-derived count

First implementation must not add optional fields unless a visible requirement cannot be met with existing contract.

Field truth table:

```text
assistant duration row
  allowed when AssistantWork has duration field
  forbidden when duration would be inferred from client render time

shell full transcript
  allowed when Rust provides UI-safe command transcript
  fallback to outputSummary block when only outputSummary exists

context compaction notice
  allowed when Rust provides structured notice code
  fallback to normal notice rendering when only body exists

attachment thumbnail
  allowed when frontend has safe preview reference
  fallback to image/file chip when only metadata exists
```

## Technical Architecture Audit

Current implementation facts that affect the plan:

- `ConversationTurnUserMessage` currently has no `attachments` field.
- `UserMessageAppendedEvent` currently carries `content` and `MessageMetadata`, but not the original `ConversationAttachmentReference[]`.
- `MessagePart::Image`, `MessagePart::Video`, and `MessagePart::File` do not preserve attachment `name`; text-only attachment references can be rendered into prompt text. Historical attachment UI therefore cannot be reconstructed safely from `MessageContent`.
- `NoticeSegment` currently has only `body`. Stable context compaction rendering needs a structured notice code.
- `ProcessDiffFile` currently exposes `preview: Option<UiSafeText>`, not parsed old/new line metadata.
- `ProcessStepDetail::Command` currently exposes `command`, `output`, `exitCode`, and `durationMs`; it has no terminal spans or stdout/stderr stream split.
- `AssistantWork` currently has no `startedAt`, `completedAt`, or `durationMs`.
- `shared/state/ui-store.ts` currently has no evidence collapse map.

Architectural decisions:

- PR 1 may reshape frontend evidence rendering with the existing command and diff fields.
- PR 1 must treat parsed diff lines as a frontend view model derived from `ProcessDiffFile.preview`.
- If `preview` lacks unified diff hunk metadata, the UI must hide old/new line numbers instead of inventing them.
- Terminal coloring is a future contract extension. First version renders plain UI-safe output.
- Historical attachments require event and projection plumbing before frontend rendering.
- Context compaction divider requires `notice.code`; until that lands, normal notices remain normal notices.
- Assistant-level duration remains hidden until Rust exposes duration fields.

Contract additions must follow this path:

```text
harness-contracts public type
-> schema export / contract tests
-> event or read-model payload projection
-> conversation worktree projector
-> frontend Zod schema
-> feature component
-> component/story/e2e coverage
```

## Task 1: Build Codex-Style Story Fixture

**Files:**

- Create: `apps/desktop/src/features/conversation/timeline/conversation-evidence-fixtures.ts`
- Modify: `apps/desktop/src/features/conversation/timeline/conversation-timeline.stories.tsx`
- Test: `apps/desktop/src/features/conversation/timeline/conversation-timeline.test.tsx`

- [ ] **Step 1: Add a full evidence fixture**

Create a `codexStyleEvidenceTurns` fixture with one user turn and one assistant work tree.

The fixture must include:

- user text in Chinese
- assistant progress text
- one `process` segment
- one `fileEdit` step
- one `diff` step with at least one added line and one removed line
- one `command` step with `exitCode: 1`
- one completed `command` step
- one `toolGroup` segment with completed and failed attempts
- one `notice` segment for context compaction
- one final `text` segment

- [ ] **Step 2: Add Storybook story**

Add story name:

```text
CodexEvidenceFlow
```

The story must render dark theme and should use the exact same component path as production `ConversationTimeline`.

- [ ] **Step 3: Add component assertion**

Test assertions:

```text
renders edited file status
renders diff filename
renders added and removed line counters
renders shell command
renders exit code 1
renders context compaction notice
renders final assistant answer
```

- [ ] **Step 4: Run focused test**

Run:

```bash
pnpm -C apps/desktop test src/features/conversation/timeline/conversation-timeline.test.tsx
```

Expected: PASS.

## Task 2: Split ProcessPanel Into Renderer Units

**Files:**

- Modify: `apps/desktop/src/features/conversation/timeline/process-panel.tsx`
- Create: `apps/desktop/src/features/conversation/timeline/process-step-row.tsx`
- Create: `apps/desktop/src/features/conversation/timeline/process-status-row.tsx`
- Test: `apps/desktop/src/features/conversation/timeline/conversation-timeline.test.tsx`

- [ ] **Step 1: Move step rendering out of ProcessPanel**

`ProcessPanel` remains responsible for:

```text
sort steps by order
render segment summary
render ordered ProcessStepRow list
```

`ProcessPanel` must not contain switch branches for command, diff, tool, artifact after this task.

- [ ] **Step 2: Add ProcessStatusRow**

`ProcessStatusRow` props:

```ts
type ProcessStatusRowProps = {
  collapsible?: boolean
  countLabel?: string
  durationMs?: number
  icon: LucideIcon
  open?: boolean
  status: ProcessStep['status']
  title: string
}
```

Rendering:

- file edit: `FilePenLine`
- file read: `FileText`
- file search: `Search`
- command: `Terminal`
- tool: `Wrench`
- artifact: `Image` or `FileText`
- reasoning/activity: `CircleDot`
- failed: use destructive status text
- running: use warning status dot

- [ ] **Step 3: Add ProcessStepRow**

`ProcessStepRow` maps `ProcessStep.kind` and `ProcessStep.detail.type` to status row plus detail component.

Rules:

- `withheld` status renders only a muted withheld message.
- `command` detail renders `CommandEvidenceBlock`.
- `diff` detail renders `DiffEvidenceBlock`.
- `tool` detail renders a compact inline row.
- `artifact` detail renders existing `ArtifactImagePreview` when image.
- `activity` detail renders count and summary inline.

- [ ] **Step 4: Preserve current user-visible behavior**

Existing tests for command, diff, tool, artifact should still pass before visual polish.

Run:

```bash
pnpm -C apps/desktop test src/features/conversation/timeline/conversation-timeline.test.tsx
```

Expected: PASS.

## Task 3: Implement CommandEvidenceBlock

**Files:**

- Create: `apps/desktop/src/features/conversation/timeline/command-evidence-block.tsx`
- Modify: `apps/desktop/src/features/conversation/timeline/process-step-row.tsx`
- Modify: `apps/desktop/src/shared/i18n/locales/en-US.ts`
- Modify: `apps/desktop/src/shared/i18n/locales/zh-CN.ts`
- Test: `apps/desktop/src/features/conversation/timeline/conversation-timeline.test.tsx`

- [ ] **Step 1: Add command block component**

Props:

```ts
type CommandEvidenceBlockProps = {
  command: string
  durationMs?: number
  exitCode?: number
  output?: string
}
```

Required rendering:

```text
Header: Shell + copy button
Command line: $ {command}
Output viewport: output
Footer: duration + exit code
```

- [ ] **Step 2: Add copy behavior**

Copy text format:

```text
$ {command}

{output}

exit {exitCode}
duration {durationMs} ms
```

If `output` is absent, omit the blank output section.

- [ ] **Step 3: Add localization keys**

English:

```text
timeline.commandEvidence.shell = Shell
timeline.commandEvidence.copy = Copy command output
timeline.commandEvidence.exitCode = exit {{code}}
timeline.commandEvidence.duration = {{duration}} ms
```

Chinese:

```text
timeline.commandEvidence.shell = Shell
timeline.commandEvidence.copy = 复制命令输出
timeline.commandEvidence.exitCode = 退出码 {{code}}
timeline.commandEvidence.duration = {{duration}} ms
```

- [ ] **Step 4: Test command block**

Assertions:

```text
shows "$ pnpm -C apps/desktop test -- SkillsPage"
shows "退出码 1" in zh-CN
uses internal scroll region for long output
copy button has accessible name
does not color output by regex when terminal spans are absent
```

Run:

```bash
pnpm -C apps/desktop test src/features/conversation/timeline/conversation-timeline.test.tsx
```

Expected: PASS.

## Task 4: Implement DiffEvidenceBlock

**Files:**

- Create: `apps/desktop/src/features/conversation/timeline/diff-evidence-block.tsx`
- Modify: `apps/desktop/src/features/conversation/timeline/process-step-row.tsx`
- Modify: `apps/desktop/src/features/conversation/DiffViewer.tsx`
- Modify: `apps/desktop/src/features/conversation/DiffPreview.tsx`
- Modify: `apps/desktop/src/shared/i18n/locales/en-US.ts`
- Modify: `apps/desktop/src/shared/i18n/locales/zh-CN.ts`
- Test: `apps/desktop/src/features/conversation/ConversationComponents.test.tsx`
- Test: `apps/desktop/src/features/conversation/timeline/conversation-timeline.test.tsx`

- [ ] **Step 1: Add diff evidence component**

Props:

```ts
type DiffEvidenceBlockProps = {
  addedLineCount: number
  filename: string
  lines: DiffEvidenceLine[]
  maxVisibleLines?: number
  removedLineCount: number
}

type DiffEvidenceLine = {
  content: string
  newLineNumber?: number
  oldLineNumber?: number
  prefix: '+' | '-' | ' '
  type: 'added' | 'removed' | 'context'
}
```

- [ ] **Step 2: Parse line types without losing visible prefixes**

Line parsing rules:

```text
content line starts with "+" -> added
content line starts with "-" -> removed
content line starts with " " -> context
metadata line starts with "+++", "---", or "@@" -> parse as diff metadata, not content
display prefix in its own column
display content without duplicated prefix
preserve oldLineNumber/newLineNumber when available
```

- [ ] **Step 3: Render Codex-style header**

Header content:

```text
{filename} +{addedLineCount} -{removedLineCount} [copy icon]
```

Rules:

- filename truncates.
- counters never wrap.
- copy button uses lucide `Copy`.
- copy button has tooltip and `aria-label`.

- [ ] **Step 4: Render stable gutters**

Columns:

```text
line number: width 44px
prefix: width 20px
content: minmax(0, 1fr)
```

Line backgrounds:

```text
added: bg-success/10
removed: bg-destructive/10
context: transparent
```

Syntax:

```text
infer language from filename extension
use Shiki through cached/lazy helper
fallback to plain monospace when language or highlighter is unavailable
```

If `ProcessDiffFile.preview` does not include `@@` hunk metadata, render the diff without old/new line numbers. Do not use array indexes as line numbers.

- [ ] **Step 5: Keep old imports working**

`DiffViewer` can delegate to `DiffEvidenceBlock` or share its parsing helper. Existing imports must compile.

- [ ] **Step 6: Run focused tests**

Run:

```bash
pnpm -C apps/desktop test src/features/conversation/ConversationComponents.test.tsx src/features/conversation/timeline/conversation-timeline.test.tsx
```

Expected: PASS.

## Task 5: Rework Assistant Flow Layout

**Files:**

- Modify: `apps/desktop/src/features/conversation/timeline/assistant-work-view.tsx`
- Modify: `apps/desktop/src/features/conversation/timeline/conversation-turn-view.tsx`
- Modify: `apps/desktop/src/features/conversation/timeline/assistant-text-segment-view.tsx`
- Modify: `apps/desktop/src/features/conversation/timeline/notice-segment-view.tsx`
- Create: `apps/desktop/src/features/conversation/timeline/context-compaction-notice.tsx`
- Test: `apps/desktop/src/features/conversation/timeline/conversation-timeline.test.tsx`

- [ ] **Step 1: Make assistant flow document-like**

`AssistantWorkView` layout:

```text
section max-w-[86%]
  assistant meta row
  segment stream grid gap-3
```

Do not wrap the whole assistant answer in a card.

- [ ] **Step 2: Render assistant status duration row**

If `assistant.status === 'running'`, show running row.

If `assistant.status === 'failed'`, show failed row.

If `assistant.status === 'complete'`, no large success banner.

Duration rule:

```text
show duration only when the contract exposes startedAt/completedAt/durationMs
do not infer assistant duration from Date.now()
do not render fake elapsed time for fixture-only parity
```

- [ ] **Step 3: Render context compaction notice**

Map structured context compaction notices to `ContextCompactionNotice`.

Preferred source:

```text
notice.code === 'contextCompacted'
```

Fallback:

```text
If no structured code exists, render as normal NoticeSegmentView.
Do not detect compaction through localized body text.
```

Visual:

```text
horizontal line  text  horizontal line
```

No card background.

- [ ] **Step 4: Keep Markdown clean**

`assistant-text-segment-view.tsx` should continue using `MarkdownMessage`.

Rules:

- raw HTML remains disabled.
- inline code remains grey capsule.
- links remain primary-colored.

- [ ] **Step 5: Run tests**

Run:

```bash
pnpm -C apps/desktop test src/features/conversation/timeline/conversation-timeline.test.tsx
```

Expected: PASS.

## Task 5A: Add Structured Notice Code Contract

**Files:**

- Modify: `crates/jyowo-harness-contracts/src/events/messages.rs`
- Modify: `crates/jyowo-harness-contracts/src/conversation.rs`
- Modify: `crates/jyowo-harness-contracts/tests/m1_contracts.rs`
- Modify: `crates/jyowo-harness-journal/src/conversation_read_model.rs`
- Modify: `crates/jyowo-harness-journal/src/conversation_worktree_projector.rs`
- Modify: `apps/desktop/src/shared/events/run-event-schema.ts`
- Modify: `apps/desktop/src/shared/events/run-event-schema.test.ts`
- Modify: `apps/desktop/src/shared/tauri/commands.ts`
- Modify: `apps/desktop/src/shared/tauri/commands.test.ts`

- [ ] **Step 1: Extend notice event contract**

Add an optional structured code to assistant notices:

```rust
#[serde(default, skip_serializing_if = "Option::is_none")]
pub code: Option<AssistantNoticeCode>
```

First code:

```text
contextCompacted
```

Rules:

- `code` is not localized text.
- Existing notices without code keep deserializing.
- Unknown future codes must not crash the frontend; render normal notice.

- [ ] **Step 2: Project code into worktree notice**

`NoticeSegment` gets the same optional code.

`conversation_read_model` includes the code in `assistant.notice` payload.

`conversation_worktree_projector` maps payload `code` into `NoticeSegment.code`.

- [ ] **Step 3: Update frontend schemas**

Update both:

```text
shared/events/run-event-schema.ts
shared/tauri/commands.ts
```

Test:

```text
assistant.notice accepts code: "contextCompacted"
unknown notice code renders as normal notice
contextCompacted renders ContextCompactionNotice
```

- [ ] **Step 4: Run focused tests**

Run:

```bash
cargo test -p jyowo-harness-contracts conversation_worktree --test m1_contracts
cargo test -p jyowo-harness-journal --test conversation_worktree_projector
pnpm -C apps/desktop test src/shared/events/run-event-schema.test.ts src/shared/tauri/commands.test.ts src/features/conversation/timeline/conversation-timeline.test.tsx
```

Expected: all PASS.

## Task 6: Rework Tool Group Rendering

**Files:**

- Modify: `apps/desktop/src/features/conversation/timeline/tool-group-segment-view.tsx`
- Modify: `apps/desktop/src/features/conversation/timeline/tool-attempt-row.tsx`
- Create: `apps/desktop/src/features/conversation/timeline/tool-evidence-summary.tsx`
- Test: `apps/desktop/src/features/conversation/timeline/conversation-timeline.test.tsx`

- [ ] **Step 1: Add tool group summary**

Summary derives from attempts:

```text
completedCount
failedCount
runningCount
waitingPermissionCount
```

Labels:

```text
已运行 n 条工具
失败 n 条
等待权限 n 条
```

Use existing i18n structure and add English equivalents.

- [ ] **Step 2: Collapse low-signal completed attempts**

Default open rules:

```text
failed attempt: open
waitingPermission attempt: open
running attempt: open
completed attempt with permission: collapsed after permission resolved
completed attempt without failure: collapsed when there are more than 2 attempts
```

Collapsed rows still show tool name and status.

- [ ] **Step 3: Keep permissions nested**

`PermissionInlinePanel` remains inside `ToolAttemptRow`.

Test must assert:

```text
permission request appears under owning tool row
permission does not render as top-level timeline item
```

- [ ] **Step 4: Run tests**

Run:

```bash
pnpm -C apps/desktop test src/features/conversation/timeline/conversation-timeline.test.tsx
```

Expected: PASS.

## Task 7: Add Historical User Attachments To Worktree

**Files:**

- Modify: `crates/jyowo-harness-contracts/src/conversation.rs`
- Modify: `crates/jyowo-harness-contracts/src/events/messages.rs`
- Modify: `crates/jyowo-harness-contracts/src/schema_export.rs`
- Modify: `crates/jyowo-harness-contracts/tests/m1_contracts.rs`
- Modify: `crates/jyowo-harness-contracts/tests/fixtures/conversation_worktree_page.json`
- Modify: `crates/jyowo-harness-session/src/session.rs`
- Modify: `crates/jyowo-harness-session/src/turn.rs`
- Modify: `crates/jyowo-harness-engine/src/turn.rs`
- Modify: `crates/jyowo-harness-sdk/src/harness.rs`
- Modify: `crates/jyowo-harness-journal/src/conversation_read_model.rs`
- Modify: `crates/jyowo-harness-journal/src/conversation_worktree_projector.rs`
- Modify: `crates/jyowo-harness-journal/tests/conversation_worktree_projector.rs`
- Modify: `apps/desktop/src/shared/events/run-event-schema.ts`
- Modify: `apps/desktop/src/shared/events/run-event-schema.test.ts`
- Modify: `apps/desktop/src/shared/tauri/commands.ts`
- Modify: `apps/desktop/src/shared/tauri/commands.test.ts`
- Create: `apps/desktop/src/features/conversation/timeline/user-attachment-strip.tsx`
- Modify: `apps/desktop/src/features/conversation/timeline/conversation-turn-view.tsx`
- Test: `apps/desktop/src/features/conversation/timeline/conversation-timeline.test.tsx`

- [ ] **Step 1: Extend Rust contract**

Add to `ConversationTurnUserMessage`:

```rust
#[serde(default, skip_serializing_if = "Vec::is_empty")]
pub attachments: Vec<ConversationAttachmentReference>,
```

Use existing `ConversationAttachmentReference`.

- [ ] **Step 2: Project attachments from user message event**

Do not reconstruct historical attachments from `MessageContent`.

Reason:

```text
MessagePart::Image / Video / File keep mime type and blob ref.
They do not preserve the original attachment name.
Text-only attachment references may only exist inside rendered prompt text.
```

Required backend path:

```text
ConversationTurnInput.attachments
-> UserMessageAppendedEvent.attachments
-> conversation_read_model assistant/user event payload
-> conversation_worktree_projector user_message_from_event
-> ConversationTurnUserMessage.attachments
```

Rules:

- invalid attachment item is ignored only if the raw event already failed safe validation upstream
- projected attachments include metadata only
- no file content is included
- event ref remains unchanged
- blob refs remain backend-owned and are not converted into frontend-readable file paths

- [ ] **Step 2B: Plumb attachments through run creation**

Update both run paths:

```text
crates/jyowo-harness-session/src/turn.rs
crates/jyowo-harness-engine/src/turn.rs
```

`submit_conversation_turn` and `run_turn_parts_with_client_message_id` must preserve original `ConversationAttachmentReference[]` or an equivalent safe metadata carrier until the user message event is emitted.

Do not serialize attachment metadata into `MessageMetadata.labels` as ad hoc JSON.

- [ ] **Step 3: Extend frontend Zod schema**

`conversationTurnUserMessageSchema` gets:

```ts
attachments: z.array(attachmentReferenceSchema).optional()
```

Type consumers treat missing as empty.

- [ ] **Step 4: Render attachment strip**

`UserAttachmentStrip` props:

```ts
type UserAttachmentStripProps = {
  attachments: AttachmentReference[]
}
```

Rendering:

- image MIME type with safe preview reference: thumbnail
- image MIME type without safe preview reference: image-style file chip
- non-image: file chip with name and size
- no blob content loaded
- fixed thumbnail size: 72px desktop, 56px compact
- attachment strip uses internal horizontal overflow for long lists
- thumbnail has explicit width, height, and alt text from safe filename

Do not call a blob fetch command from the timeline renderer. A thumbnail requires a dedicated safe preview contract.

- [ ] **Step 5: Run contract and frontend tests**

Run:

```bash
cargo test -p jyowo-harness-contracts conversation_worktree --test m1_contracts
cargo test -p jyowo-harness-contracts schema_export --test m1_contracts
cargo test -p jyowo-harness-session
cargo test -p jyowo-harness-engine
cargo test -p jyowo-harness-sdk
cargo test -p jyowo-harness-journal --test conversation_worktree_projector
pnpm -C apps/desktop test src/shared/events/run-event-schema.test.ts src/shared/tauri/commands.test.ts src/features/conversation/timeline/conversation-timeline.test.tsx
```

Expected: all PASS.

## Task 8A: Add Visual Regression Coverage

**Files:**

- Modify: `apps/desktop/src/features/conversation/timeline/conversation-timeline.stories.tsx`
- Modify: `apps/desktop/src/features/conversation/timeline/conversation-timeline.test.tsx`
- Modify: `apps/desktop/src/app/App.test.tsx` only if route smoke needs fixture access
- Test: Storybook build
- Test: Playwright smoke if story-to-route fixture is exposed through web mock runtime

- [ ] **Step 1: Storybook state matrix**

Stories required:

```text
CodexEvidenceFlow
CodexEvidenceRunning
CodexEvidenceFailedCommand
CodexEvidenceLargeDiff
CodexEvidencePermissionPending
CodexEvidenceContextCompacted
```

- [ ] **Step 2: Add DOM shape tests**

Assertions:

```text
diff block has filename header
diff block has line number gutter
diff block line numbers come from parsed old/new line metadata
diff block does not classify +++/--- metadata as changed content
shell block has terminal surface
shell block has exit code footer
completed command history can collapse
failed command remains visible
context compaction divider renders as line-text-line
user bubble uses neutral surface, not primary brand fill
```

- [ ] **Step 3: Add theme coverage**

Required screenshot targets:

```text
CodexEvidenceFlow system
CodexEvidenceFlow light
CodexEvidenceFlow dark
CodexEvidenceFailedCommand dark
CodexEvidenceLargeDiff light
CodexEvidencePermissionPending dark
```

Assertions:

```text
no visible overlap
evidence blocks are non-empty
copy buttons are visible on focus
diff and shell scroll regions stay inside their blocks
main timeline remains the primary surface
long filenames and long commands truncate or scroll without widening the column
```

- [ ] **Step 4: Run Storybook build**

Run:

```bash
pnpm -C apps/desktop build-storybook
```

Expected: PASS.

- [ ] **Step 5: Run desktop tests**

Run:

```bash
pnpm check:desktop
```

Expected: PASS.

## Task 8B: Add Attachment Visual Regression Coverage

**Files:**

- Modify: `apps/desktop/src/features/conversation/timeline/conversation-timeline.stories.tsx`
- Modify: `apps/desktop/src/features/conversation/timeline/conversation-timeline.test.tsx`
- Test: Storybook build

- [ ] **Step 1: Add attachment stories after Task 7 lands**

Stories required:

```text
CodexEvidenceAttachmentsMetadataOnly
CodexEvidenceAttachmentsWithSafePreview
```

- [ ] **Step 2: Add DOM shape tests**

Assertions:

```text
user attachments render before user body
attachment strip aligns with the user bubble right edge
metadata-only image attachment renders as chip
safe preview image attachment renders as thumbnail
file chip shows name and size
timeline renderer does not fetch blob content
more than four attachments use internal horizontal overflow
```

- [ ] **Step 3: Run focused tests**

Run:

```bash
pnpm -C apps/desktop test src/features/conversation/timeline/conversation-timeline.test.tsx
pnpm -C apps/desktop build-storybook
```

Expected: PASS.

## Task 9: Update Frontend Documentation

**Files:**

- Modify: `docs/frontend/frontend-product-ux.md`
- Modify: `docs/frontend/frontend-engineering.md`
- Modify: `docs/frontend/frontend-quality.md`
- Test: docs gate

- [ ] **Step 1: Product UX doc**

Add explicit rule:

```text
Conversation canvas renders assistant work as narrative text plus execution evidence blocks.
Execution evidence blocks include status rows, diff blocks, command blocks, tool rows, permission panels, artifact previews, and compaction notices.
Raw events remain in Activity, Details, Replay, and Raw JSON.
```

- [ ] **Step 2: Engineering doc**

Add component ownership:

```text
features/conversation/timeline owns evidence blocks:
CommandEvidenceBlock
DiffEvidenceBlock
ProcessStatusRow
ToolEvidenceSummary
UserAttachmentStrip
ContextCompactionNotice
```

- [ ] **Step 3: Quality doc**

Add required coverage:

```text
Codex-style evidence conversation fixture
dark-theme evidence screenshot
large diff
failed command
historical attachments
collapsed completed history
```

- [ ] **Step 4: Run docs gate**

Run:

```bash
pnpm check:docs
```

Expected: PASS.

## Task 10: Final Verification

**Files:**

- No new source files.
- Verify all touched files.

- [ ] **Step 1: Drift guards**

Run:

```bash
rg -n "RunEvent" apps/desktop/src/features/conversation/timeline
```

Expected: no output.

Run:

```bash
rg -n "get_conversation\\.messages|ConversationBlockRow|blocks\\?: ConversationTurn\\[\\]|pendingPermissionBlocks|Tool error withheld from conversation timeline" apps/desktop/src/features/conversation apps/desktop/src/shared/tauri -g '!**/*.test.ts' -g '!**/*.test.tsx' -g '!**/*.stories.tsx'
```

Expected: no production canvas regressions.

- [ ] **Step 2: Frontend gate**

Run:

```bash
pnpm check:desktop
```

Expected: PASS.

- [ ] **Step 3: Rust gate when Task 7 is included**

Run:

```bash
pnpm check:rust
```

Expected: PASS.

- [ ] **Step 4: Full gate for merge**

Run:

```bash
pnpm check
```

Expected: PASS.

## Acceptance Checklist

The implementation is accepted only when all items below are true:

- The main canvas visually reads as a conversation, not an event log.
- User message bubble matches the right-aligned Codex posture.
- User message bubble uses a neutral quiet surface, not a primary brand fill.
- Assistant work renders as document flow.
- File edit status rows are visible.
- Adjacent low-signal statuses can aggregate into compact rows.
- Diff evidence block shows filename, `+n -n`, line gutter, added/removed backgrounds, copy affordance.
- Diff line numbers and changed-line classification are derived from parsed diff metadata, not array indexes.
- Shell evidence block shows `Shell`, command line, output viewport, duration, exit code.
- Failed command remains expanded.
- Completed low-signal commands can collapse.
- Tool permissions remain nested under tool attempts.
- Context compaction notice is a centered divider.
- Context compaction notice is keyed by structured `notice.code`, not localized body text.
- Historical attachments render from worktree data, not composer draft state.
- Historical attachments are carried through user message event/projection, not reconstructed from `MessageContent`.
- System, light, and dark screenshots preserve the same hierarchy.
- Dark mode matches the Codex reference posture without hardcoding Codex colors.
- Light mode remains usable and token-driven.
- No feature component hardcodes raw hex product colors.
- `features/conversation/timeline` does not import `RunEvent`.
- `pnpm check:desktop` passes.
- `pnpm check:rust` passes if contract fields changed.

## Recommended PR Split

PR 1: Frontend evidence renderer plus structured notice code.

Included tasks:

- Task 1
- Task 2
- Task 3
- Task 4
- Task 5
- Task 5A
- Task 6
- Task 8A

PR 2: Historical user attachments in worktree.

Included tasks:

- Task 7
- Task 8B

PR 3: Documentation and final gates.

Included tasks:

- Task 9
- Task 10

This split keeps the visual foundation reviewable while isolating the heavier attachment event/projection work.
