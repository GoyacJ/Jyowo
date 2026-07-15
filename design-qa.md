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
