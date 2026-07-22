# 任务工作台上下文重构：设计 QA

## 比较目标

参考设计：

- `/Users/goya/Library/Containers/com.tencent.xinWeChat/Data/Documents/xwechat_files/wxid_nfsvik4hifth22_541f/temp/InputTemp/3b7ef449-4079-4219-9704-e14722513907.png`
- `/Users/goya/Library/Containers/com.tencent.xinWeChat/Data/Documents/xwechat_files/wxid_nfsvik4hifth22_541f/temp/InputTemp/451bc809-6418-4f4d-bf39-95b24c0d2ffe.png`
- `/Users/goya/Library/Containers/com.tencent.xinWeChat/Data/Documents/xwechat_files/wxid_nfsvik4hifth22_541f/temp/InputTemp/b34dc124-563a-48f9-a6d2-d57e15172348.png`
- `/Users/goya/Library/Containers/com.tencent.xinWeChat/Data/Documents/xwechat_files/wxid_nfsvik4hifth22_541f/temp/InputTemp/93f8a17f-c12d-4c54-9a30-acd041ba7b5c.png`
- `/Users/goya/Library/Containers/com.tencent.xinWeChat/Data/Documents/xwechat_files/wxid_nfsvik4hifth22_541f/temp/InputTemp/0e3ac6f3-7ac4-4b89-a575-2f498195669f.png`

实现截图：

- `/Users/goya/Repo/Git/Jyowo/artifacts/design-qa/task-workbench-desktop-1440x900.png`
- `/Users/goya/Repo/Git/Jyowo/artifacts/design-qa/task-workbench-overlay-900x900.png`
- `/Users/goya/Repo/Git/Jyowo/artifacts/design-qa/task-workbench-fullscreen-690x844.png`
- `/Users/goya/Repo/Git/Jyowo/artifacts/design-qa/task-workbench-light-900x900.png`
- `/Users/goya/Repo/Git/Jyowo/artifacts/design-qa/task-workbench-object-preview-1440x900.png`
- `/Users/goya/Repo/Git/Jyowo/artifacts/design-qa/task-workbench-object-overlay-900x900.png`
- `/Users/goya/Repo/Git/Jyowo/artifacts/design-qa/task-workbench-object-fullscreen-690x844.png`

组合比较证据：

- 全图：`/Users/goya/Repo/Git/Jyowo/artifacts/design-qa/task-workbench-comparison-board.png`
- 局部：`/Users/goya/Repo/Git/Jyowo/artifacts/design-qa/task-workbench-comparison-focus.png`

## 视口与状态

| 视口 | 布局状态 | 检查内容 |
|---|---|---|
| 1440 × 900 | 停靠 | 会话、悬浮摘要、右侧工作台并列；对象标签；Diff 内容 |
| 900 × 900 | 覆盖 | 工作台覆盖会话；悬浮摘要隐藏；关闭后恢复焦点 |
| 690 × 844 | 全屏 | 返回会话；焦点限制；关闭按钮隐藏 |
| 900 × 900 | 浅色 | 会话、消息、Composer、状态文字和边框 token |
| 1440 × 900 | 对象停靠 | 文本文件、真实 PNG、来源信息和固定标签 |
| 900 × 900 | 对象覆盖 | 文件入口打开、Esc 关闭和焦点恢复 |
| 690 × 844 | 对象全屏 | 会话 `inert`、返回会话焦点和图片预览 |

## 比较结论

### 全图

参考设计把会话作为主区，右侧工作区按需出现。实现保持同一结构，但继续使用 Jyowo 的任务页面、内容面板和设计变量，没有复刻参考图中的完整代码编辑器。该差异属于已确认的产品范围，不是设计漂移。

停靠模式保留了会话阅读宽度。右侧工作台使用独立边界和内容滚动。覆盖模式不继续压缩会话。全屏模式将返回动作提升为主入口。三档布局没有重叠、裁切或持久控件不可见问题。

### 局部

局部比较覆盖悬浮摘要和右侧工作台。实现吸收了参考设计的弱边框、低对比背景、紧凑行高、活动行高亮和按内容动态出现的结构。实现没有照搬参考图中的 Git 专属项目，而是映射为文件变更、命令、来源、环境、子智能体和产物。

工作台标题、预览标签、定位动作和关闭动作形成稳定层级。标签内容、标题和辅助信息在当前视口内没有异常换行或遮挡。

## 必查表面

### 字体与排版

实现复用 Jyowo 现有字体栈。标题、辅助信息、标签和代码内容的字号层级稳定。长标题使用截断。中文与英文没有挤压主要操作。参考图的字体并非本项目设计资产，因此不做字族逐像素复刻。

### 间距与布局

摘要卡片行距、分组间距、工作台标题栏、标签栏和内容区保持一致节奏。停靠模式不使用大面积阴影。覆盖模式通过轻量阴影表达层级。全屏模式没有残留停靠间距。

### 颜色与设计变量

实现只使用现有背景、表面、边框、前景和语义状态变量。深色界面与参考图的低对比层级一致。浅色主题经浏览器检查，正文、消息气泡、边框和 Composer 均保持可读。

### 图像与资产

该功能没有新增插画、Logo 或产品图片。图标使用现有 Lucide 图标库，没有使用 Emoji、CSS 绘图、手写 SVG 或占位图替代参考资产。参考截图仅用于结构与交互方向，不作为运行时资产。

### 文案与内容

固定文案已进入中英文资源。对象标题来自任务内容。数量、运行、失败和新内容提示可以同时表达，不依赖单一颜色。全屏使用“返回会话”，滚动状态使用“回到最新”“有新内容”或消息数量。

## 交互与无障碍

浏览器已验证：

- 从悬浮摘要打开工作台；
- 关闭工作台后焦点返回原摘要入口；
- 双击固定标签；
- 固定标签保留，预览标签被新对象替换；
- “在会话中定位”聚焦并短暂高亮来源事件；
- 悬浮摘要收起与展开；
- 覆盖模式隐藏悬浮摘要；
- 全屏模式限制焦点并显示“返回会话”；
- Paused 状态新增消息保持阅读位置，并显示“1 条新消息”；
- 点击新消息提示恢复 Following；
- Following 状态新增消息保持最新内容可见；
- Paused 状态流式增长只显示“有新内容”，不增加消息数量；
- 浅色主题切换后主要界面保持可读。
- 用户消息附件、file、image 和普通 artifact 可从会话打开；
- 图片以真实 Object URL 加载，文本文件按媒体类型解码；
- overlay 中按 Esc 关闭后，焦点返回来源事件的“打开文件”按钮；
- 任务工作区容器在 719/720/1039/1040px 分别切换全屏、覆盖、覆盖、停靠；
- 1040px 容器下面板宽度限制为 400px，不挤占会话最小阅读宽度。

控制台检查结果：无 `error` 或 `warn`。

复现信息：

- 检查日期：2026-07-14；
- 启动命令：`pnpm -C apps/desktop storybook --host 127.0.0.1`；
- 工作台场景：`http://localhost:6006/iframe.html?id=tasks-task-workspace--open-workbench&viewMode=story`；
- 对象预览场景：`http://localhost:6006/iframe.html?id=tasks-task-workspace--object-previews&viewMode=story`；
- 滚动场景：`http://localhost:6006/iframe.html?id=tasks-task-workspace--scroll-following&viewMode=story`；
- 控制台检查：场景完成后读取 `error` 和 `warn`，结果均为空；
- 自动测试负责可重复验证状态转换；截图负责验证可见布局和主题。

## P0/P1/P2 修复历史

| 级别 | 先前问题 | 修复 | 修复后证据 |
|---|---|---|---|
| P1 | Storybook 工作台场景缺少 `loadTaskEvents`，对象面板不能稳定打开 | 补齐测试客户端能力 | 1440、900、690 三个浏览器场景均可打开工作台 |
| P1 | 用户显式上滚时可能仍处于程序滚动保护窗口 | 用户滚动立即终止程序保护 | Paused 时新增消息和流式增长均保持阅读位置 |
| P1 | 距底部 24px 内向上滚动可能被阈值立即恢复为 Following | 将向上方向判断置于底部阈值之前 | 新增控制器测试，向上 10px 仍进入 Paused |
| P1 | “在会话中定位”后仍处于 Following，后续更新可能拉回底部 | 定位完成后显式进入 Paused | 浏览器定位后显示“跳到最新消息”；来源事件保持聚焦与高亮 |
| P1 | 悬浮摘要打开对象时会丢失具体对象标题 | 保留摘要 Target 的对象标题 | 工作台标题和活动摘要项一致 |
| P2 | 数量存在时会遮蔽运行或失败状态 | 数量与状态同时展示 | 摘要行同时显示对象数量和运行状态 |
| P2 | 缺少可重复的滚动跟随浏览器场景 | 增加 `ScrollFollowing` Storybook 场景 | Following、Paused、未读数量和流式更新均已复核 |

当前已实施范围内没有未解决的 P0、P1 或 P2 设计缺陷。计划中的大文件、大 Diff、连续 ResizeObserver 压力和标签溢出增强属于未纳入本次验收的后续范围，不等同于未修复缺陷。

## 自动验证

- `pnpm check:frontend:fast`：69 个测试文件通过，721 项测试通过。
- `pnpm check:design-tokens`：通过。本次涉及文件没有新增设计变量警告。
- `pnpm check:daemon-protocol`：通过。
- `cargo test -p jyowo-harness-journal --features sqlite preserves_object_identity_for_generated_artifacts`：1 项通过。
- 500 条时间线虚拟化已有组件测试；200% 缩放和完整减少动态效果浏览器验证尚未执行。

## 后续优化

- P3：可以为浅色工作台对象面板补充独立视觉回归截图。目前浅色证据覆盖会话和滚动状态场景，不影响本次验收。

## 过程流修复验收

视觉真值：

- `/Users/goya/Library/Containers/com.tencent.xinWeChat/Data/Documents/xwechat_files/wxid_nfsvik4hifth22_541f/temp/InputTemp/fb0b28ff-38e2-47f5-87d5-1f9e8ed9fde9.png`

实现证据：

- 主视口：`/Users/goya/Repo/Git/Jyowo/docs/design-qa/task-workspace-reference-process-flow.png`
- 窄屏：`/Users/goya/Repo/Git/Jyowo/docs/design-qa/task-workspace-reference-process-flow-narrow.png`
- 全图比较：`/Users/goya/Repo/Git/Jyowo/docs/design-qa/task-workspace-reference-process-flow-comparison.png`
- 过程区局部比较：`/Users/goya/Repo/Git/Jyowo/docs/design-qa/task-workspace-reference-process-flow-focus-comparison.png`

视口与状态：

| 视口 | 状态 | 结果 |
|---|---|---|
| 893 × 755 | 运行中，工具组折叠，Diff 摘要可打开 | 过程顺序、内容密度、Composer 和底部状态条均正常 |
| 690 × 844 | 运行中，中文长文案 | 无横向溢出；时间线 `clientWidth` 与 `scrollWidth` 均为 642px |

全图比较显示，会话正文已经从生命周期日志改为“阶段说明 → 工具活动摘要 → 变更 → 阶段结果 → 当前动作”。底部状态只保留一个运行入口。实现延续项目既有的对象工作台设计，因此长 Diff 由摘要卡片进入工作台，不在会话中复刻完整代码编辑器。

局部比较显示，工具活动使用低对比、单行、可折叠摘要；正文与工具组的阅读顺序和参考图一致。折叠后不再出现 requested、started、completed 三条重复记录。

字体沿用项目字体栈。正文为 15px，工具摘要和辅助状态保持次级层级。中文与英文数量文案没有异常断行。

时间线分组间距已压缩。工具摘要、Diff 卡片、Composer 和状态条之间没有重叠。893px 和 690px 视口均无水平滚动。

颜色全部使用现有背景、前景、边框和语义状态变量。工具摘要降低对比度，正文保持主对比度。图标使用现有 Lucide 资产，没有新增图片、Emoji、手写 SVG 或占位图。

固定文案已进入中英文资源。工具摘要表达对象和数量；阶段正文表达结果与下一步；状态条同时显示当前动作、步骤、最近变更和耗时。

交互检查：鼠标可展开和收起工具组；`summary` 获得键盘焦点后可用 Enter 收起；展开后每次工具调用显示结果和耗时；窄屏可返回最新消息。控制台 `error` 和 `warn` 均为空。

本轮 P0/P1/P2 修复历史：

| 级别 | 问题 | 修复 | 验收结果 |
|---|---|---|---|
| P0 | 生命周期事件占据正文，真实执行过程不可读 | 普通 run、task、workspace 生命周期事件退出会话正文；失败、阻塞、权限事件保留 | 正文只保留用户输入、阶段说明、工具活动、产物和结果 |
| P0 | 同一次工具调用被拆成 requested、started、completed 多条记录 | daemon 投影按 `toolUseId` 聚合状态和结果 | 每次调用只出现一次；状态、摘要、结果和耗时随事件更新 |
| P1 | 工具输出是技术日志，缺少参考图中的语义过程 | 增加工具活动折叠组和文件、编辑、命令等语义摘要 | 折叠态显示“读取了 1 个文件 · 运行了 2 个命令”；展开态保留细节 |
| P1 | RunSegment 和底部状态重复展示“运行中/已完成” | 删除段标题；底部状态条作为唯一运行入口 | 会话中没有重复运行标题；状态条不遮挡正文或 Composer |
| P1 | 助手长时间无阶段反馈 | 系统提示约束多步骤任务前说明目标，一组操作后说明结果和下一步 | Storybook 过程按阶段说明和工具组交替展示 |
| P2 | 时间线间距过大，信息密度偏离参考图 | 压缩分组和行间距 | 893 × 755 内可连续阅读完整主过程 |
| P2 | 屏幕阅读器只播报笼统更新 | live region 改为播报最新事件的语义摘要 | 当前动作可被具体播报 |
| P2 | 缺少可复现的参考过程场景 | 增加 `Tasks/Task workspace / Reference process flow` | 浏览器可在固定数据和视口下复核 |

当前没有未解决的 P0、P1 或 P2。

## 右侧上下文与命令正文验收

参考截图：

- `/Users/goya/Library/Containers/com.tencent.xinWeChat/Data/Documents/xwechat_files/wxid_nfsvik4hifth22_541f/temp/InputTemp/943155b0-8dda-4390-bb34-3a5c9a7dd068.png`
- `/Users/goya/Library/Containers/com.tencent.xinWeChat/Data/Documents/xwechat_files/wxid_nfsvik4hifth22_541f/temp/InputTemp/e3667fdf-9b93-4a01-8c22-424c85ed6228.png`
- `/var/folders/dl/236yt2x97h1288s49g2m5j4h0000gn/T/codex-clipboard-89a80386-73d4-4de8-b0ff-0000630da8be.png`

实现证据：

- 右侧上下文收起态：`/Users/goya/Repo/Git/Jyowo/artifacts/design-qa/task-context-inline-commands-collapsed-893x755.png`
- 右侧上下文展开态：`/Users/goya/Repo/Git/Jyowo/artifacts/design-qa/task-context-inline-commands-expanded-893x755.png`
- 文件工作台停靠态：`/Users/goya/Repo/Git/Jyowo/artifacts/design-qa/task-file-workbench-1200x800.png`
- 参考与实现组合图：`/Users/goya/Repo/Git/Jyowo/artifacts/design-qa/task-context-inline-commands-comparison.png`

| 视口 | 状态 | 结果 |
|---|---|---|
| 893 × 755 | 任务上下文固定右侧并展开 | 会话宽度随上下文收缩；时间线没有压入右栏；命令和输出在正文内展开 |
| 1200 × 800 | 点击文件更改 | 任务上下文隐藏；文件工作台停靠在右侧；会话与工作台没有重叠 |
| 690 × 844 | 窄屏 | 任务上下文隐藏；`body.clientWidth` 与 `body.scrollWidth` 均为 690px |

本轮修复：

| 级别 | 问题 | 修复 | 验收结果 |
|---|---|---|---|
| P0 | 命令只显示抽象工具记录，正文没有命令行和真实输出 | daemon 与 live projection 增加命令、输出字段；命令工具组默认展开 Shell 转录 | 正文显示 `$ command`、输出、运行状态和耗时；命令不打开右侧工作台 |
| P0 | 右侧上下文展开后，时间线保持旧的固有宽度并压入右栏 | 时间线两层 flex 子项增加 `min-width: 0` | 893px 展开态中会话区域和右栏边界一致，无覆盖 |
| P1 | 任务上下文位于阅读列内部，不能稳定固定在右侧 | 上下文改为阅读列 sibling，并占满任务区域高度 | 上下文在桌面视口持续固定于右侧；工作台出现时自动隐藏 |
| P1 | 命令、权限、环境、错误、审计等事件会打开无意义侧栏 | 侧栏 Target 限制为 file、artifact、diff、image/source、subagent | 命令与普通工具事件只在正文交互；旧 session 的无效 Target 也不会显示侧栏 |
| P2 | 右侧上下文混入命令、审计和错误，环境行也被误做成可点击入口 | 上下文只保留文件、来源、产物、静态环境、Subagent 和 Agent Team | 文件与 Agent 类条目可打开；环境保持静态信息；错误不进入上下文栏 |

浏览器检查覆盖上下文收起、展开、文件工作台替换、命令折叠交互和窄屏溢出。应用运行时没有 `error` 或 `warn`；Storybook 外壳有一条与功能无关的 `/favicon.ico` 404。

自动验证：

- `pnpm -C apps/desktop typecheck`：通过。
- `pnpm -C apps/desktop lint`：通过。
- `pnpm -C apps/desktop test`：69 个测试文件、719 项测试通过。
- `pnpm check:daemon-protocol`：通过。
- `cargo test -p jyowo-harness-contracts`：通过。
- `cargo test -p jyowo-harness-journal --features sqlite`：通过。
- `cargo fmt --check`：通过。

当前没有未解决的 P0、P1 或 P2。

## 环境信息卡片视觉验收

视觉真值：

- `/var/folders/dl/236yt2x97h1288s49g2m5j4h0000gn/T/codex-clipboard-161a816b-e08d-4607-bf7b-dbbbc498c5c3.png`

实现截图：

- `/Users/goya/Repo/Git/Jyowo/artifacts/design-qa/task-context-environment-panel-893x755.png`
- `/Users/goya/Repo/Git/Jyowo/artifacts/design-qa/task-context-environment-panel-1200x800.png`
- `/Users/goya/Repo/Git/Jyowo/artifacts/design-qa/task-context-file-workbench-1200x800.png`
- `/Users/goya/Repo/Git/Jyowo/artifacts/design-qa/task-context-responsive-690x844.png`

组合比较证据：

- 全图：`/Users/goya/Repo/Git/Jyowo/artifacts/design-qa/task-context-environment-panel-full-comparison.png`
- 右栏局部：`/Users/goya/Repo/Git/Jyowo/artifacts/design-qa/task-context-environment-panel-comparison.png`

视口与状态：

| 视口 | 状态 | 结果 |
|---|---|---|
| 893 × 755 | 环境信息默认展开 | 右侧独立悬浮卡片；会话与卡片无覆盖 |
| 1200 × 800 | 完整环境、来源、子智能体数据 | 卡片宽 304px、高 366px；分组、状态和统计可读 |
| 1200 × 800 | 点击来源文件 | 环境信息卡片隐藏；文件工作台替代右栏；关闭后卡片恢复 |
| 690 × 844 | 窄屏 | 环境信息卡片隐藏；`innerWidth` 与 `scrollWidth` 均为 690px |

全图比较确认卡片固定在任务区域右上方，并与参考图保持同类的独立圆角表面、低对比边框和紧凑层级。参考截图包含 Git 操作；实现按本产品数据映射为文件更改、本地工作区、来源和子智能体，没有复制不适用的 Git 命令。

右栏局部比较确认卡片宽度、圆角、行高、分隔、图标容器和选中行背景接近参考。实现使用 `surface-raised`，使卡片与页面背景的层级接近参考图。新增和删除统计分别使用完成与失败状态色。

字体沿用项目 Inter/system 字体栈。标题、分组、主信息和辅助信息保持四级层级。长路径与标题使用截断，不挤压状态和计数。

间距使用 10px 外边距、44px 标题栏、40px 行高和 8px 行内间距。卡片宽度在桌面端为 248–304px。阴影只表达浮层关系，没有形成重边框。

颜色全部来自现有 `surface-raised`、`row-muted`、`border` 和状态 token。图标继续使用项目现有 Lucide 库。没有新增 Emoji、CSS 绘图、手写 SVG 或占位资产。固定文案进入中英文资源；来源行显示真实文件名，变更行显示 `+新增/-删除`。

本轮比较历史：

| 级别 | 先前问题 | 修复 | 修复后证据 |
|---|---|---|---|
| P1 | 任务上下文仍像贴边栏，缺少参考图的悬浮卡片层级 | 改为带外边距、圆角、轻边框和阴影的独立右侧卡片 | 893 与 1200 截图中卡片均与页面边缘分离 |
| P1 | 默认状态不能直接看到环境信息 | 默认展开，保留显式收起按钮 | 页面首次加载即显示完整分组 |
| P2 | 卡片背景与页面层级不足 | 使用 `surface-raised/95` | 局部组合图中卡片与参考图均形成可辨识的抬升表面 |
| P2 | 回归场景只有文件更改，无法验证完整结构 | Story 补齐 workspace、来源、文件和 subagent | 1200 截图覆盖三个分组和全部目标类型 |

控制台 `error` 与 `warn` 均为空。当前没有未解决的 P0、P1 或 P2。无需额外局部放大；304px 右栏裁剪已能清楚检查全部文字、图标、间距与状态色。

final result: passed

## 输入框快捷面板验收

视觉真值：

- 斜杠指令：`/var/folders/r6/yxptvvk91mn385_y6sw55nx40000gn/T/codex-clipboard-419b1860-6782-4548-897a-3633c3665097.png`（1576 × 912 px）。
- 引用选择：`/var/folders/r6/yxptvvk91mn385_y6sw55nx40000gn/T/codex-clipboard-0acbaaca-5b2c-488b-85c3-2e87f7e2eb80.png`（1576 × 912 px）。

实现证据：

- 斜杠面板：`/Users/goya/Repo/Git/Jyowo/artifacts/design-qa/composer-slash-panel-1440x900.png`。
- 引用面板：`/Users/goya/Repo/Git/Jyowo/artifacts/design-qa/composer-reference-panel-1440x900.png`。
- 工具栏引用搜索：`/Users/goya/Repo/Git/Jyowo/artifacts/design-qa/composer-toolbar-reference-panel-1440x900.png`。
- 480px 窄屏：`/Users/goya/Repo/Git/Jyowo/artifacts/design-qa/composer-slash-panel-480x800.png`。
- 斜杠全图比较：`/Users/goya/Repo/Git/Jyowo/artifacts/design-qa/composer-slash-reference-vs-implementation.png`。
- 引用全图比较：`/Users/goya/Repo/Git/Jyowo/artifacts/design-qa/composer-reference-reference-vs-implementation.png`。
- 斜杠局部比较：`/Users/goya/Repo/Git/Jyowo/artifacts/design-qa/composer-slash-focused-comparison.png`。
- 引用局部比较：`/Users/goya/Repo/Git/Jyowo/artifacts/design-qa/composer-reference-focused-comparison.png`。

实现截图使用 1440 × 900 CSS px 视口；窄屏使用 480 × 800 CSS px。浏览器截图已按 CSS 像素保存。检查状态包含 `/` 初始态、`/rev` 筛选态、编辑器内 `@`、工具栏 `@` 搜索态和窄屏态。

全图比较确认面板固定在输入框上方，并与输入框同宽。局部比较确认候选行统一使用图标、名称、说明和整行选中态。`@` 面板按对象类型分组；编辑器内触发没有第二个搜索输入，工具栏触发时搜索输入自动聚焦。

必查表面：

- 字体与排版：沿用现有字体栈；命令、名称、说明分层稳定，长路径和说明均截断，不产生异常换行。
- 间距与布局：面板、列表和键盘提示使用统一内边距；桌面和 480px 宽度均无重叠或横向溢出。
- 颜色与设计变量：只使用现有语义变量和 `shadow-deep`；没有新增原始色值或任意阴影。
- 图像与资产：类型图标使用项目现有 Lucide 图标库；没有新增位图、手写 SVG、Emoji 或 CSS 图形。
- 文案与内容：四条指令和七类引用均有中英文名称；命令说明、分组名称、空状态、结果数和键盘提示均通过 i18next 提供。
- 图标：指令和引用类型采用不同语义图标，尺寸和线宽一致。
- 响应式与可访问性：列表使用 `listbox/option`；编辑器通过 `aria-controls`、`aria-expanded` 和 `aria-activedescendant` 关联候选项；支持方向键、Enter、Tab、Esc；480px 下 `clientWidth` 与 `scrollWidth` 均为 480px。

浏览器交互验证覆盖 `/rev` 过滤、Tab 插入 `/review `、编辑器内 `@`、工具栏搜索自动聚焦、Esc 关闭和点击外部关闭。浏览器控制台 `error` 与 `warn` 均为空。定向 Vitest 共 55 项通过，TypeScript 类型检查和设计变量检查通过。前端全量测试中的 8 项失败来自既有 Node `localStorage` 测试环境问题。`pnpm check:quick` 被仓库内既有超限测试文件阻断，本次新增测试文件不在超限列表中。

本轮没有未解决的 P0、P1 或 P2 设计缺陷。

final result: passed

## 侧栏双击重命名与顶部间距验收（2026-07-18）

视觉真值：

- `/var/folders/r6/yxptvvk91mn385_y6sw55nx40000gn/T/codex-clipboard-698af8e3-4357-426c-bbcd-33c6ba0364e1.png`

实现证据：

- 默认态：`/Users/goya/Repo/Git/Jyowo/artifacts/design-qa/sidebar-inline-rename-final-2048x1280.png`
- 双击编辑态：`/Users/goya/Repo/Git/Jyowo/artifacts/design-qa/sidebar-inline-rename-editing-2048x1280.png`
- 全图组合比较：`/Users/goya/Repo/Git/Jyowo/artifacts/design-qa/sidebar-reference-vs-final-4096x1280.png`
- 侧栏局部比较：`/Users/goya/Repo/Git/Jyowo/artifacts/design-qa/sidebar-focused-reference-vs-final-720x700.png`

视口为 2048 × 1280，深色主题，已选择并运行中的会话。参考图包含 macOS 原生窗口边框；浏览器实现图不包含原生边框，因此全图按内容高度归一化，侧栏局部按相同逻辑宽度比较。

全图比较确认现有阅读列、环境卡片、Composer 和状态栏没有被本次侧栏调整影响。局部比较确认新建会话按钮上方的 48px 空白已移除，仅保留 8px 组件内边距；按钮、定时任务和分组列表的横向对齐保持不变。

双击会话名称后，标题在原行切换为输入框并自动聚焦、全选。回车和失焦保存；Esc 取消。编辑时行内操作按钮隐藏，避免挤压输入区域。折叠图标侧栏不进入编辑态。

必查表面：字体继续使用项目既有 Inter/system 字体栈与 13px 侧栏层级；间距使用 8px 设计节奏，输入框保持 24px 高度；颜色全部来自 `background`、`input`、`ring` 和状态 token；状态图标继续使用现有 Lucide 资产，没有新增图片、CSS 绘图或手写 SVG；文案复用现有中英文“会话名称”资源。

交互检查覆盖双击进入编辑、焦点与选区、Esc 取消和默认态恢复。浏览器控制台没有 `error`、`warn` 或布局异常。

比较历史：

| 级别 | 先前问题 | 修复 | 修复后证据 |
|---|---|---|---|
| P2 | 删除顶部占位后，新建会话按钮紧贴浏览器内容边缘 | 顶部动作容器增加 8px 纵向内边距 | 按钮 `top=8px`，圆角完整，侧栏仍无大块空白 |

当前没有未解决的 P0、P1 或 P2。定向测试 2 个文件、24 项测试通过；类型检查和 Biome 检查通过。完整前端检查被本功能范围外的 `PendingQuestionForm.test.tsx` 既有可访问名称断言拦截；`pnpm check:quick` 被仓库既有超长测试文件拦截于 test-architecture gate。

final result: passed

## 会话名称顶部定位验收

视觉真值：

- 目标位置：`/var/folders/r6/yxptvvk91mn385_y6sw55nx40000gn/T/codex-clipboard-bf313926-27d2-48b8-aa2a-b0c3a2b3cd06.png`
- 调整前状态：`/var/folders/r6/yxptvvk91mn385_y6sw55nx40000gn/T/codex-clipboard-b337444f-6a5e-4a99-86a2-5e540916e84d.png`

实现证据：

- 浏览器截图：`/Users/goya/Repo/Git/Jyowo/artifacts/design-qa/conversation-title-topbar-1920x1280.png`
- 全图组合比较：`/Users/goya/Repo/Git/Jyowo/artifacts/design-qa/conversation-title-position-comparison.jpg`

视口为 1920 × 1280，状态为已选择、运行中的会话。会话名称显示在右侧区域顶部栏左侧，相对右侧区域偏移为 x=32px、y=16px。正文区域不再重复显示标题。690 × 844 窄屏复核中标题保持可见，`body.clientWidth` 与 `body.scrollWidth` 均为 690px。

全图比较已覆盖整体位置、顶部栏层级和右侧区域对齐。标题在同一画面中可读，不需要额外局部放大。

字体沿用现有字体栈，使用 14px 半粗标题；间距沿用顶部栏的 52px 高度和设计 token；颜色沿用前景色；没有新增图片或替代资产；标题内容直接来自当前会话投影并支持截断。浏览器控制台无 `error` 或 `warn`。

首次比较没有发现 P0、P1 或 P2 差异，因此没有修复迭代。相关组件测试 15 项通过，类型检查和 Biome 检查通过。完整前端测试被当前 Node 26.5.0 的无 `localStorage` 测试环境拦截；`pnpm check:quick` 被仓库中 11 个既有超长测试文件拦截，均与本次标题布局无关。

final result: passed

## 对话文件活动与 Diff 验收（2026-07-18）

参考截图：

- `/var/folders/r6/yxptvvk91mn385_y6sw55nx40000gn/T/codex-clipboard-ff6d951b-dc6e-4e3d-88b3-c69a82d9025b.png`
- `/var/folders/r6/yxptvvk91mn385_y6sw55nx40000gn/T/codex-clipboard-6fc7ba91-7044-4e27-902d-87ed44f90ae6.png`
- `/var/folders/r6/yxptvvk91mn385_y6sw55nx40000gn/T/codex-clipboard-5281f260-759d-439b-b38e-43a49de343f4.png`
- `/var/folders/r6/yxptvvk91mn385_y6sw55nx40000gn/T/codex-clipboard-28e722cb-84bb-4e39-9de3-055fd8dae5c2.png`

实现截图：

- 1440 × 900 内联文件活动：`/Users/goya/Repo/Git/Jyowo/artifacts/design-qa/task-file-activity-inline-final-1440x900.jpg`
- 1440 × 900 文件查看器：`/Users/goya/Repo/Git/Jyowo/artifacts/design-qa/task-file-activity-file-viewer-final-1440x900.jpg`
- 1440 × 900 Diff 工作台：`/Users/goya/Repo/Git/Jyowo/artifacts/design-qa/task-file-activity-diff-workbench-1440x900.jpg`
- 690 × 844 内联文件活动：`/Users/goya/Repo/Git/Jyowo/artifacts/design-qa/task-file-activity-inline-690x844.jpg`

组合比较证据：

- 全图：`/Users/goya/Repo/Git/Jyowo/artifacts/design-qa/comparison-inline-full.jpg`
- 文件活动与内联 Diff 局部：`/Users/goya/Repo/Git/Jyowo/artifacts/design-qa/comparison-inline-focused.jpg`
- 文件查看器：`/Users/goya/Repo/Git/Jyowo/artifacts/design-qa/comparison-file-viewer.jpg`

| 视口 | 状态 | 结果 |
|---|---|---|
| 1440 × 900 | 文件读取与编辑完成 | 活动按真实操作类型显示图标、文件名和耗时；关联产物不再作为重复时间线项出现 |
| 1440 × 900 | 内联 Diff 展开 | 文件标题、新增/删除统计、hunk、行号与增删背景均可读；内容区独立滚动 |
| 1440 × 900 | 点击读取记录 | 文件工作台打开对应快照；等宽文本和行号稳定对齐 |
| 1440 × 900 | 点击编辑记录 | Diff 工作台打开同一补丁；内联与工作台内容一致 |
| 690 × 844 | 文件活动展开 | 活动、统计和 Diff 在会话列内回流；无横向页面溢出 |

全图比较确认实现保留 Jyowo 既有浅色任务页面、右侧工作台和设计变量，没有复制参考应用的深色外壳。局部比较使用同一“文件操作完成、Diff 展开”状态，结构顺序与参考一致：操作记录、已编辑文件分组、文件标题、统计和逐行 Diff。参考图是裁剪画面，因此局部证据按内容区域归一化宽度比较，不作为整页像素复刻基准。

文件查看器继续使用既有任务工作台，不新增独立全屏编辑器。读取记录打开持久化文件快照；编辑记录打开持久化 unified diff。文件视图补充行号，长行和 Diff 内容在自身区域滚动。代码语法着色属于后续 P3，不影响文件内容、行号、选择和补丁检查。

本轮修复历史：

| 级别 | 问题 | 修复 | 验收结果 |
|---|---|---|---|
| P1 | 文件工具只有过程文字，历史任务没有可打开的文件内容或补丁 | FileRead、FileEdit、FileWrite 将文件快照或 unified diff 写入 blob，并记录关联 `source_tool_use_id` 的产物事件 | 历史投影可恢复文件与 Diff；点击对应工具记录可打开工作台 |
| P1 | 关联产物作为独立时间线项显示，和工具记录重复 | 连续工具组按 `toolUseId` 吸收关联 file/diff 产物 | 每次读取或编辑只保留一条可点击活动记录 |
| P2 | 完成态统一使用对勾，无法快速区分读取、搜索和编辑 | 完成态改用操作类型图标；运行和失败仍保留状态图标 | 文件读取与编辑可在同组内快速扫描 |
| P2 | 内联 Diff 缺少文件统计 | 根据解析后的增删行计算并显示 `+新增/-删除` | 标题行与参考层级一致，读屏同时获得完整数量描述 |
| P2 | 文件工作台只有纯文本块，没有行号 | file artifact 的工作台视图增加稳定行号列 | 文件快照可按行定位；行号不参与正文语义 |
| P2 | 窄屏可能被 Diff 长行撑宽 | Diff 表格保留内部横向滚动，活动容器允许收缩 | 690 × 844 截图无页面级横向溢出 |

浏览器交互覆盖读取记录、编辑记录、内联 Diff、文件工作台、Diff 工作台和窄屏。控制台 `error` 与 `warn` 均为空。当前没有未解决的 P0、P1 或 P2。

自动验证：

- 前端定向测试：5 个测试文件、88 项测试通过。
- `pnpm check:frontend:fast`：67 个测试文件、685 项测试通过。
- `pnpm check:daemon-protocol`：通过。
- `pnpm check:design-tokens`：通过；输出仅包含仓库已有警告。
- `pnpm check:rust:fast`：通过。
- File artifact Rust 单元测试：2 项通过。
- FileRead/FileEdit/FileWrite Rust 集成测试：35 项通过。
- Journal 投影 Rust 测试：9 项通过。
- `pnpm check:quick`：被本功能范围外的 10 个既有超长测试文件拦截于 test-architecture gate；后续前端、Rust 和协议 gate 已分别执行并通过。

final result: passed

## 右侧任务区域运行反馈与控制验收

参考截图：

- `/var/folders/r6/yxptvvk91mn385_y6sw55nx40000gn/T/codex-clipboard-5d8dca97-3cf3-49ff-9bb4-bdc1d3852a4d.png`
- `/var/folders/r6/yxptvvk91mn385_y6sw55nx40000gn/T/codex-clipboard-f7f5619c-cfb2-48ce-854c-7a7e5db55114.png`

实现证据：

- 完整运行态：`/Users/goya/Repo/Git/Jyowo/artifacts/design-qa/task-run-feedback-integrated-1920x1280.png`
- 顶部标题：`/Users/goya/Repo/Git/Jyowo/artifacts/design-qa/task-title-app-header-1920x1280.png`
- 暂停后继续：`/Users/goya/Repo/Git/Jyowo/artifacts/design-qa/task-run-paused-1920x1280.png`
- 窄屏：`/Users/goya/Repo/Git/Jyowo/artifacts/design-qa/task-run-feedback-narrow-690x844.png`

参考图与实现图在同一比较输入中检查。实现沿用原有深色背景、阅读列宽度、右侧环境卡片、Composer 和 Lucide 图标，没有引入新的视觉体系。

| 视口 | 状态 | 结果 |
|---|---|---|
| 1920 × 1280 | 运行中、当前段尚无助手输出 | 会话名称位于顶部栏左侧；阅读列不再显示标题、“已连接”或普通任务状态；时间线显示“正在思考”；Composer 显示暂停和排队 |
| 1920 × 1280 | 安全暂停完成 | Composer 显示继续；点击继续请求成功；浏览器控制台无 `error` 或 `warn` |
| 690 × 844 | 运行中 | 标题、等待反馈和暂停入口可见；`clientWidth`、`scrollWidth` 和 `body.scrollWidth` 均为 690px |

交互与状态检查：

- 暂停使用 daemon 的 `stop_run`，模式为 `safe_point`；等待安全点时显示“正在暂停”并禁用重复请求。
- 安全暂停产生 `interrupted + cancelled` 后显示继续，并发送 `continue_task`。
- 运行时仍可编辑并排队下一条消息。
- “正在思考”和“正在暂停”使用 `role=status`；动画支持减少动态效果。
- 暂停和继续均有可见文字、图标和可访问名称。

自动验证：

- `pnpm check:frontend:fast`：66 个测试文件、680 项测试通过。
- 相关组件测试：5 个测试文件、118 项测试通过。
- `pnpm check:quick`：已执行；被仓库内其他既有测试架构硬限制阻断。本次涉及的 `TaskWorkspace.test.tsx` 已拆分到 1200 行以内。

当前没有未解决的 P0、P1 或 P2 设计缺陷。

final result: passed

## 工具设置页重设计验收

旧页面证据：

- `/Users/goya/.codex/visualizations/2026/07/14/019f612a-9667-7ae1-8ff4-dbd270e46707/tools-page-audit/02-settings-tools-status.png`
- `/Users/goya/.codex/visualizations/2026/07/14/019f612a-9667-7ae1-8ff4-dbd270e46707/tools-page-audit/03-settings-tools-runtime-list.png`

实现证据：

- 桌面默认态：`/Users/goya/.codex/visualizations/2026/07/14/019f612a-9667-7ae1-8ff4-dbd270e46707/tools-page-audit-new/01-settings-tools-desktop.png`
- 高风险确认：`/Users/goya/.codex/visualizations/2026/07/14/019f612a-9667-7ae1-8ff4-dbd270e46707/tools-page-audit-new/02-high-risk-confirmation.png`
- 运行环境详情：`/Users/goya/.codex/visualizations/2026/07/14/019f612a-9667-7ae1-8ff4-dbd270e46707/tools-page-audit-new/03-runtime-details.png`
- 窄屏：`/Users/goya/.codex/visualizations/2026/07/14/019f612a-9667-7ae1-8ff4-dbd270e46707/tools-page-audit-new/04-settings-tools-narrow.png`

| 步骤 | 状态 | 结果 |
|---|---|---|
| 1. 打开设置 → 工具 | 通过 | 运行环境压缩为一行摘要；工具改为分组列表；不再出现六列表格和横向滚动 |
| 2. 搜索与筛选 | 通过 | 搜索、全部、已启用、不可用、高风险均按预期更新列表和匹配数 |
| 3. 普通工具开关 | 通过 | 关闭后状态变为“已停用”，启用数与分组数同步更新，恢复默认按钮进入可用状态 |
| 4. 高风险工具开关 | 通过 | 关闭无需确认；再次启用先显示破坏性操作说明，确认后才更新状态 |
| 5. 恢复默认 | 通过 | 工具状态恢复，按钮重新禁用 |
| 6. 运行环境详情 | 通过 | 可展开查看沙箱候选、策略和限制原因，可再次收起 |
| 7. 690 × 844 窄屏 | 通过 | `documentElement.clientWidth` 与 `scrollWidth` 均为 690px，无横向溢出 |

后端开关不是前端隐藏。设置写入项目执行覆盖配置；新任务装配工具池时使用同一 `ToolProfile` 过滤器。恢复默认会删除项目级工具覆盖并继续继承全局配置。未注册工具名由后端拒绝。

可访问性检查覆盖了工具筛选分组、具名开关、对话框标题与按钮、键盘焦点样式、语义标题层级和窄屏回流。截图与 DOM 不能证明完整 WCAG 合规；本轮没有发现可见或语义结构风险。

自动验证：

- `pnpm -C apps/desktop typecheck`：通过。
- `pnpm -C apps/desktop lint`：通过。
- `pnpm -C apps/desktop test`：69 个测试文件、716 项测试通过。
- Runtime tools Rust 测试：5 项通过。
- Tool profile/filter Rust 测试：2 项通过。
- Tauri 命令注册测试：1 项通过。
- `cargo fmt --all -- --check`：通过。
- 浏览器控制台 `error` 和 `warn`：均为空。

当前没有未解决的 P0、P1 或 P2。

final result: passed

## 全局搜索入口迁移验收

视觉真值：

- 命令弹框：`/var/folders/r6/yxptvvk91mn385_y6sw55nx40000gn/T/codex-clipboard-19bde467-af20-47c8-a24c-3982e28b2b5d.png`（1142 × 476 px）。
- 原顶部按钮：`/var/folders/r6/yxptvvk91mn385_y6sw55nx40000gn/T/codex-clipboard-d47d4411-681f-4aa3-8d00-0cf1184e35cb.png`（168 × 166 px）。

实现证据：

- 侧栏入口：`/Users/goya/Repo/Git/Jyowo/artifacts/design-qa/global-search-sidebar-implementation.png`（1280 × 720 px）。
- 弹框打开态：`/Users/goya/Repo/Git/Jyowo/artifacts/design-qa/global-search-popup-implementation.png`（1280 × 720 px）。
- 全图组合比较：`/Users/goya/Repo/Git/Jyowo/artifacts/design-qa/global-search-reference-vs-implementation.jpg`。
- 局部组合比较：`/Users/goya/Repo/Git/Jyowo/artifacts/design-qa/global-search-focused-comparison.jpg`。

视口为 1280 × 720 CSS px。浏览器设备像素比为 2；实现截图已归一化为 1280 × 720 px。状态为浅色主题、侧栏展开、无选中会话，并分别检查弹框关闭态和打开态。

全图比较确认顶部栏不再保留独立命令按钮。全局搜索位于侧栏顶部，并在新建会话之前。收起侧栏时入口只显示搜索图标。局部比较确认弹框的输入框、操作分组、默认选中项、三个命令和关闭按钮与视觉真值保持同一结构。

必查表面：

- 字体与排版：沿用现有字体栈、14px 菜单文字和 11px 快捷键提示；没有截断或异常换行。
- 间距与布局：入口使用侧栏既有 36px 行高、圆角、水平内边距和 4px 行间距；新建会话位置下移一行，无重叠或横向溢出。
- 颜色与设计变量：只使用现有前景、弱前景、行悬停和表面变量；没有新增原始色值或自定义阴影。
- 图像与资产：搜索图标使用项目现有 Lucide 图标库；没有新增位图资产、手写 SVG、Emoji 或 CSS 绘图。
- 文案与内容：新增“全局搜索 / Global search”和对应可访问名称；弹框原有命令文案保持不变。

浏览器验证覆盖点击“全局搜索”打开命令弹框、搜索输入自动聚焦、三个命令项可见和弹框关闭按钮可见。控制台 `error` 为空。自动测试覆盖入口顺序、点击打开弹框和设置命令路由。

本轮没有发现 P0、P1 或 P2 设计缺陷。首次前端全量测试受 Node 本地存储参数影响；补充隔离的 `--localstorage-file` 后，68 个测试文件、699 项测试全部通过。`pnpm check:quick` 被仓库内既有超限测试文件阻断，本次涉及文件不在超限列表中。

final result: passed
